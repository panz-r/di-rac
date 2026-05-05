import { ToolUse } from "@core/assistant-message"
import { resolveWorkspacePath } from "@core/workspace"
import { ASTAnchorBridge } from "@utils/ASTAnchorBridge"
import { stripHashes } from "@utils/line-hashing"
import { getReadablePath, isLocatedInWorkspace } from "@utils/path"
import { createToolError } from "@shared/tool-response"
import { formatResponse } from "@/core/prompts/responses"
import { telemetryService } from "@/services/telemetry"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import { showNotificationForApproval } from "../../utils"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { ToolResultUtils } from "../utils/ToolResultUtils"

export class GetFileSkeletonToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.GET_FILE_SKELETON

	constructor(private validator: ToolValidator) {}

	getDescription(block: ToolUse): string {
		const relPaths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : (block.params.path ? [block.params.path as string] : []))
		return `${block.name} ${relPaths.join(" ")}`
	}

	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		const relPaths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : (block.params.path ? [block.params.path as string] : []))
		const config = uiHelpers.getConfig()
		if (config.isSubagentExecution) {
			return
		}

		const firstPath = relPaths[0] || ""
		const sharedMessageProps = {
			tool: "getFileSkeleton",
			path: getReadablePath(config.cwd, uiHelpers.removeClosingTag(block, "paths", firstPath)),
			paths: relPaths.map((p) => getReadablePath(config.cwd, p)),
		}

		const partialMessage = JSON.stringify(sharedMessageProps)

		if (await uiHelpers.shouldAutoApproveToolWithPath(block.name, firstPath)) {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("ask", "tool")
			await uiHelpers.say("tool", partialMessage, undefined, undefined, block.partial)
		} else {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("say", "tool")
			await uiHelpers.ask("tool", partialMessage, block.partial).catch(() => {})
		}
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const relPaths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : (block.params.path ? [block.params.path as string] : []))
		const apiConfig = config.services.stateManager.getApiConfiguration()
		const currentMode = config.services.stateManager.getGlobalSettingsKey("mode")
		const provider = (currentMode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string

		if (relPaths.length === 0) {
			config.taskState.consecutiveMistakeCount++
			return await config.callbacks.sayAndCreateMissingParamError(this.name, "paths")
		}
		const absolutePaths: string[] = []
		const displayPaths: string[] = []
		const skeletons: { path: string; content: string }[] = []

		try {
			for (const relPath of relPaths) {
				const pathResult = resolveWorkspacePath(config, relPath, "GetFileSkeletonToolHandler.execute")
				const { absolutePath, displayPath } =
					typeof pathResult === "string" ? { absolutePath: pathResult, displayPath: relPath } : pathResult
				absolutePaths.push(absolutePath)
				displayPaths.push(displayPath)
			}
			const parseResults = await Promise.all(
				absolutePaths.map(async (absPath) => {
					try {
						return await ASTAnchorBridge.getFileSkeleton(
							absPath,
							config.services.diracIgnoreController,
							config.ulid,
							{ showCallGraph: true },
							config.services.analyzer,
						)
					} catch (error) {
						return `Error parsing ${absPath}: ${error instanceof Error ? error.message : String(error)}`
					}
				}),
			)

			for (let i = 0; i < relPaths.length; i++) {
				const parseResult = parseResults[i]
				const displayPath = getReadablePath(config.cwd, displayPaths[i])
				const content = parseResult || `No definitions found in ${relPaths[i]}`
				skeletons.push({ path: displayPath, content: content })
			}
		} catch (error) {
			config.taskState.consecutiveMistakeCount++
			const errorMessage = error instanceof Error ? error.message : String(error)
			return formatResponse.formatToolErrorForLLM(createToolError("tool.internalError", `Error extracting skeleton: ${errorMessage}`, "recoverable"))
		}
		const result = skeletons.map((s) => `--- ${s.path} ---\nStable Anchors are provided with each line.\n ${s.content}`).join("\n\n")

		if (
			skeletons.some(
				(s) =>
					s.content.includes("No definitions found") ||
					s.content.includes("Unsupported file type") ||
					s.content.includes("Could not parse") ||
					s.content.includes("Error parsing"),
			)
		) {
			config.taskState.consecutiveMistakeCount++
		} else {
			config.taskState.consecutiveMistakeCount = 0
		}

		const sharedMessageProps = {
			tool: "getFileSkeleton",
			paths: displayPaths.map((p) => getReadablePath(config.cwd, p)),
			skeletons: skeletons.map((s) => ({ ...s, content: stripHashes(s.content) })),
			operationIsLocatedInWorkspace: (await Promise.all(relPaths.map((p) => isLocatedInWorkspace(p)))).every(Boolean),
		}

		const completeMessage = JSON.stringify(sharedMessageProps)

		const shouldAutoApprove =
			config.isSubagentExecution ||
			(await Promise.all(relPaths.map((p) => config.callbacks.shouldAutoApproveToolWithPath(block.name, p)))).every(Boolean)

		if (shouldAutoApprove) {
			if (!config.isSubagentExecution) {
				await config.callbacks.removeLastPartialMessageIfExistsWithType("ask", "tool")
				await config.callbacks.say("tool", completeMessage, undefined, undefined, false)
			}

			telemetryService.captureToolUsage(
				config.ulid,
				block.name,
				config.api.getModel().id,
				provider,
				true,
				true,
				undefined,
				block.isNativeToolCall,
			)
		} else {
			const notificationMessage = `di wants to extract file skeleton from ${relPaths.length} file(s)`
			showNotificationForApproval(notificationMessage, config.autoApprovalSettings.enableNotifications)

			await config.callbacks.removeLastPartialMessageIfExistsWithType("say", "tool")
			await config.callbacks.removeLastPartialMessageIfExistsWithType("ask", "tool")

			const { didApprove } = await ToolResultUtils.askApprovalAndPushFeedback("tool", completeMessage, config)
			if (!didApprove) {
				telemetryService.captureToolUsage(
					config.ulid,
					block.name,
					config.api.getModel().id,
					provider,
					false,
					false,
					undefined,
					block.isNativeToolCall,
				)
				return formatResponse.toolDenied()
			}
			telemetryService.captureToolUsage(
				config.ulid,
				block.name,
				config.api.getModel().id,
				provider,
				false,
				true,
				undefined,
				block.isNativeToolCall,
			)
		}

		return result
	}
}
