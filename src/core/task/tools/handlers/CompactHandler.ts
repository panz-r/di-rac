import type { ToolUse } from "@core/assistant-message"
import { getHookModelContext } from "@core/hooks/hook-model-context"
import { getHooksEnabledSafe } from "@core/hooks/hooks-utils"
import { executePreCompactHookWithCleanup, HookCancellationError } from "@core/hooks/precompact-executor"
import { continuationPrompt } from "@core/prompts/contextManagement"
import { formatResponse } from "@core/prompts/responses"
import { resolveWorkspacePath } from "@core/workspace"
import { extractFileContent } from "@integrations/misc/extract-file-content"
import { DiracSayTool } from "@shared/ExtensionMessage"
import { Logger } from "@/shared/services/Logger"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import type { IPartialBlockHandler, IToolHandler } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"

export class CompactHandler implements IToolHandler, IPartialBlockHandler {
	readonly name = DiracDefaultTool.COMPACT

	constructor(private validator: ToolValidator) {}

	getDescription(block: ToolUse): string {
		return `[${block.name}]`
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		try {
			const context: string | undefined = block.params.context
			const requiredFiles: string[] | undefined = block.params.required_files

			if (!context) {
				config.taskState.consecutiveMistakeCount++
				return await config.callbacks.sayAndCreateMissingParamError(this.name, "context")
			}

			config.taskState.consecutiveMistakeCount = 0

			let hookContextModification: string | undefined

			const hooksEnabled = getHooksEnabledSafe(config.services.stateManager.getGlobalSettingsKey("hooksEnabled"))
			if (hooksEnabled) {
				try {
					const apiHistory = config.messageState.getApiConversationHistory()

					const result = await executePreCompactHookWithCleanup({
						taskId: config.taskId,
						ulid: config.ulid,
						modelContext: getHookModelContext(config.api, config.services.stateManager),
						workspaceRoot: config.cwd,
						apiConversationHistory: apiHistory,
						conversationHistoryDeletedRange: config.taskState.conversationHistoryDeletedRange,
						contextManager: config.services.contextManager,
						diracMessages: config.messageState.getDiracMessages(),
						messageStateHandler: config.messageState,
						compactionStrategy: "voluntary-compact",
						say: config.callbacks.say,
						setActiveHookExecution: async (hookExecution) => {
							if (hookExecution) {
								await config.callbacks.setActiveHookExecution(hookExecution)
							}
						},
						clearActiveHookExecution: config.callbacks.clearActiveHookExecution,
						postStateToWebview: config.callbacks.postStateToWebview,
						taskState: config.taskState,
						cancelTask: config.callbacks.cancelTask,
						hooksEnabled,
					})

					if (result.contextModification) {
						hookContextModification = result.contextModification
						Logger.log(`[PreCompact] Hook provided context modification for task ${config.taskId}`)
					}
				} catch (error) {
					if (error instanceof HookCancellationError) {
						await config.callbacks.say(
							"error",
							"Context compaction was cancelled by PreCompact hook. Task has been aborted.",
						)
						return "Context compaction was cancelled. Task has been aborted."
					}

					await config.callbacks.say(
						"error",
						`PreCompact hook failed, continuing with compaction: ${error instanceof Error ? error.message : String(error)}`,
					)
					Logger.error("[PreCompact] Hook execution failed, continuing with compaction:", error)
				}
			}

			// Show completed summary in tool UI
			const completeMessage = JSON.stringify({
				tool: "compact",
				content: context,
			} satisfies DiracSayTool)

			await config.callbacks.removeLastPartialMessageIfExistsWithType("say", "tool")
			await config.callbacks.say("tool", completeMessage, undefined, undefined, false)

			// Read required files back into context
			const loadedFilePaths: string[] = []
			let fileContents = ""
			const filePaths: string[] = requiredFiles || []

			if (filePaths.length > 0) {
				let filesProcessed = 0
				let filesLoaded = 0
				let totalChars = 0
				const MAX_FILES_LOADED = 8
				const MAX_FILES_PROCESSED = 10
				const MAX_CHARS = 100_000

				const loadedFiles = new Set<string>()

				for (const relPath of filePaths) {
					const normalizedPath = relPath.toLowerCase()
					if (loadedFiles.has(normalizedPath)) {
						continue
					}
					loadedFiles.add(normalizedPath)

					filesProcessed++
					if (filesProcessed > MAX_FILES_PROCESSED) {
						break
					}

					const accessValidation = this.validator.checkDiracIgnorePath(relPath)
					if (!accessValidation.ok) {
						continue
					}

					if (await config.callbacks.shouldAutoApproveToolWithPath(DiracDefaultTool.FILE_READ, relPath)) {
						try {
							const pathResult = resolveWorkspacePath(config, relPath, "CompactHandler")
							const { absolutePath, displayPath } =
								typeof pathResult === "string" ? { absolutePath: pathResult, displayPath: relPath } : pathResult

							const fileContent = await extractFileContent(absolutePath, false)

							if (totalChars + fileContent.text.length > MAX_CHARS) {
								break
							}

							await config.services.fileContextTracker.trackFileContext(relPath, "file_mentioned")

							fileContents += `\n\n<file_content path="${displayPath}">\n${fileContent.text}\n</file_content>`
							loadedFilePaths.push(displayPath)

							totalChars += fileContent.text.length
							filesLoaded++

							if (filesLoaded >= MAX_FILES_LOADED) {
								break
							}
						} catch (error) {
							Logger.error(`Failed to read ${relPath} during compact:`, error)
						}
					}
				}
			}

			if (fileContents) {
				const fileMentionString = loadedFilePaths.map((path) => `'${path}'`).join(", ") + " (see below for file content)"
				fileContents =
					`\n\nThe following files were automatically read based on the required_files parameter: ${fileMentionString}. These are the latest versions of these files - you should reference them directly and not re-read them:` +
					fileContents
			}

			let toolResultContent = continuationPrompt(context) + fileContents

			if (hookContextModification) {
				toolResultContent += `\n\n[Context Modification from PreCompact Hook]\n${hookContextModification}`
			}

			const toolResult = formatResponse.toolResult(toolResultContent)

			// Truncate the entire history (keep: "none") — voluntary compaction starts fresh
			const apiConversationHistory = config.messageState.getApiConversationHistory()
			config.taskState.conversationHistoryDeletedRange = config.services.contextManager.getNextTruncationRange(
				apiConversationHistory,
				config.taskState.conversationHistoryDeletedRange,
				"none",
			)
			await config.messageState.saveDiracMessagesAndUpdateHistory()

			config.taskState.currentlySummarizing = true

			return toolResult
		} catch (error) {
			return `Error compacting context: ${(error as Error).message}`
		}
	}

	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		const context = block.params.context || ""

		if (!context) {
			return
		}

		const partialMessage = JSON.stringify({
			tool: "compact",
			content: uiHelpers.removeClosingTag(block, "context", context),
		} satisfies DiracSayTool)

		await uiHelpers.say("tool", partialMessage, undefined, undefined, block.partial)
	}
}
