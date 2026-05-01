import { ToolUse } from "@core/assistant-message"
import { resolveWorkspacePath } from "@core/workspace"
import { FileAnchorIndex } from "@shared/utils/file-anchor-index"
import { formatLineWithHash } from "@utils/line-hashing"
import { getReadablePath } from "@utils/path"
import { createToolError } from "@shared/tool-response"
import * as fs from "fs/promises"
import * as path from "path"
import { formatResponse } from "@/core/prompts/responses"
import { HostProvider } from "@/hosts/host-provider"
import { getDiagnosticsProviders } from "@/integrations/diagnostics/getDiagnosticsProviders"
import { SymbolIndexService, SymbolLocation } from "@/services/symbol-index/SymbolIndexService"
import { telemetryService } from "@/services/telemetry"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import { showNotificationForApproval } from "../../utils"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { ToolResultUtils } from "../utils/ToolResultUtils"
import { setTimeout as setTimeoutPromise } from "node:timers/promises"

export class RenameSymbolToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.RENAME_SYMBOL
	public diagnosticsTimeoutMs = 1500
	public diagnosticsDelayMs = 500

	constructor(
		private validator: ToolValidator,
		private readonly useLinterOnlyForSyntax: boolean = false,
	) {}

	getDescription(block: ToolUse): string {
		const existingSymbol = block.params.existing_symbol as string
		const newSymbol = block.params.new_symbol as string
		const paths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : [])
		return `[${block.name} for '${existingSymbol}' to '${newSymbol}' in ${paths.map((p) => `'${p}'`).join(", ")}]`
	}

	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		const existingSymbol = uiHelpers.removeClosingTag(block, "existing_symbol", (block.params.existing_symbol as string) || "")
		const newSymbol = uiHelpers.removeClosingTag(block, "new_symbol", (block.params.new_symbol as string) || "")
		const paths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : [])

		const config = uiHelpers.getConfig()
		if (config.isSubagentExecution) {
			return
		}

		const sharedMessageProps = {
			tool: "renameSymbol",
			existing_symbol: existingSymbol,
			new_symbol: newSymbol,
			paths: paths.map((p) => getReadablePath(config.cwd, p)),
		}

		const partialMessage = JSON.stringify(sharedMessageProps)

		const firstPath = paths[0] || ""
		if (await uiHelpers.shouldAutoApproveToolWithPath(DiracDefaultTool.EDIT_FILE, firstPath)) {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("ask", "tool")
			await uiHelpers.say("tool", partialMessage, undefined, undefined, block.partial)
		} else {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("say", "tool")
			await uiHelpers.ask("tool", partialMessage, block.partial).catch(() => {})
		}
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const existingSymbol = block.params.existing_symbol as string
		const newSymbol = block.params.new_symbol as string
		const relPaths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : [])

		if (!existingSymbol || !newSymbol || relPaths.length === 0) {
			config.taskState.consecutiveMistakeCount++
			return await config.callbacks.sayAndCreateMissingParamError(
				this.name,
				!existingSymbol ? "existing_symbol" : !newSymbol ? "new_symbol" : "paths",
			)
		}

		const apiConfig = config.services.stateManager.getApiConfiguration()
		const currentMode = config.services.stateManager.getGlobalSettingsKey("mode")
		const provider = (currentMode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string

		try {
			const indexService = SymbolIndexService.getInstance()
			const projectRoot = config.workspaceManager?.getPrimaryRoot()?.path || config.cwd

			// Ensure the index is initialized for the current project root
			if (indexService.getProjectRoot() !== projectRoot) {
				await indexService.initialize(projectRoot)
			}

			const absolutePaths = relPaths.map((p) => {
				const pathResult = resolveWorkspacePath(config, p, "RenameSymbolToolHandler.execute")
				const absPath = typeof pathResult === "string" ? pathResult : pathResult.absolutePath
				return path.resolve(absPath)
			})

			// Update index for files in the requested paths (up to a limit)
			if (absolutePaths.length <= 100) {
				for (const absPath of absolutePaths) {
					try {
						const stats = await fs.stat(absPath)
						if (stats.isFile()) {
							await indexService.updateFile(absPath)
						}
					} catch (e) {
						// Skip if path doesn't exist
					}
				}
			}

			// Find all occurrences of the symbol
			const allLocations = indexService.getSymbols(existingSymbol)

			// Filter locations by requested paths
			const relevantLocations = allLocations.filter((loc) => {
				const absLocPath = path.join(projectRoot, loc.path)
				return absolutePaths.some(
					(requestedPath) => absLocPath === requestedPath || absLocPath.startsWith(requestedPath + path.sep),
				)
			})

			if (relevantLocations.length === 0) {
				return `No occurrences of symbol '${existingSymbol}' found in the specified paths.`
			}

			// Group locations by file
			const locationsByFile = new Map<string, SymbolLocation[]>()
			for (const loc of relevantLocations) {
				const absPath = path.join(projectRoot, loc.path)
				let fileLocs = locationsByFile.get(absPath)
				if (!fileLocs) {
					fileLocs = []
					locationsByFile.set(absPath, fileLocs)
				}
				fileLocs.push(loc)
			}

			const fileResults: Array<{
				absolutePath: string
				displayPath: string
				originalContent: string
				finalContent: string
				diff: string
				replacementCount: number
			}> = []

			for (const [absolutePath, locs] of locationsByFile.entries()) {
				const displayPath = path.relative(config.cwd, absolutePath)
				await HostProvider.workspace.saveOpenDocumentIfDirty({ filePath: absolutePath })
				const originalContent = await fs.readFile(absolutePath, "utf8")
				const originalLines = originalContent.split(/\r?\n/)
				const originalHashes = new FileAnchorIndex(originalLines).getAllHashes()

				// Sort locations from bottom to top, and right to left on the same line
				const sortedLocs = [...locs].sort((a, b) => {
					if (b.startLine !== a.startLine) {
						return b.startLine - a.startLine
					}
					return b.startColumn - a.startColumn
				})

				const currentLines = [...originalLines]
				let replacementCount = 0
				const seenLines = new Set<number>()

				for (const loc of sortedLocs) {
					const lineIndex = loc.startLine
					const line = currentLines[lineIndex]
					const actualName = line.slice(loc.startColumn, loc.endColumn)

					if (actualName === existingSymbol) {
						const newLine = line.slice(0, loc.startColumn) + newSymbol + line.slice(loc.endColumn)
						currentLines[lineIndex] = newLine
						replacementCount++

						// For diffing, we want to show the context around the change
						if (!seenLines.has(lineIndex)) {
							seenLines.add(lineIndex)
						}
					}
				}

				const finalContent = currentLines.join("\n")

				// Generate diff blocks for the file
				const fileDiffs: string[] = []
				const sortedChangedLines = Array.from(seenLines).sort((a, b) => a - b)
				
				let i = 0
				while (i < sortedChangedLines.length) {
					let start = sortedChangedLines[i]
					let end = start
					while (i + 1 < sortedChangedLines.length && sortedChangedLines[i + 1] <= end + 3) {
						end = sortedChangedLines[++i]
					}
					
					const diffBlock = this.getDiffBlock(
						originalLines,
						originalHashes,
						currentLines,
						start,
						end
					)
					fileDiffs.push(diffBlock)
					i++
				}

				fileResults.push({
					absolutePath,
					displayPath,
					originalContent,
					finalContent,
					diff: fileDiffs.join("\n\n---\n\n"),
					replacementCount,
				})
			}

			// Handle approval
			const totalReplacements = fileResults.reduce((sum, fr) => sum + fr.replacementCount, 0)
			const allDiffs = fileResults.map((fr) => `*** Update File: ${fr.displayPath}\n\n${fr.diff}`).join("\n\n")

			const completeMessage = JSON.stringify({
				tool: "renameSymbol",
				existing_symbol: existingSymbol,
				new_symbol: newSymbol,
				total_replacements: totalReplacements,
				files_affected: fileResults.length,
				editSummaries: fileResults.map((fr) => ({
					path: fr.displayPath,
					edits: [{ additions: fr.replacementCount, deletions: fr.replacementCount }],
				})),
				diff: allDiffs,
			})

			const shouldAutoApprove =
				config.isSubagentExecution ||
				(await Promise.all(
					fileResults.map((fr) => config.callbacks.shouldAutoApproveToolWithPath(DiracDefaultTool.EDIT_FILE, fr.displayPath)),
				).then((results) => results.every(Boolean)))

			if (!shouldAutoApprove) {
				const notificationMessage = `Dirac wants to rename symbol '${existingSymbol}' to '${newSymbol}' (${totalReplacements} occurrences in ${fileResults.length} files)`
				showNotificationForApproval(notificationMessage, config.autoApprovalSettings.enableNotifications)

				await config.callbacks.removeLastPartialMessageIfExistsWithType("say", "tool")
				await config.callbacks.removeLastPartialMessageIfExistsWithType("ask", "tool")
				const { didApprove } = await ToolResultUtils.askApprovalAndPushFeedback("tool", completeMessage, config)
				if (!didApprove) {
					telemetryService.captureToolUsage(
						config.ulid,
						this.name,
						config.api.getModel().id,
						provider,
						false,
						false,
						undefined,
						block.isNativeToolCall,
					)
					return formatResponse.toolDenied()
				}
			} else {
				if (!config.isSubagentExecution) {
					await config.callbacks.removeLastPartialMessageIfExistsWithType("ask", "tool")
					await config.callbacks.say("tool", completeMessage, undefined, undefined, false)
				}
			}

			// Diagnostics setup
			const providers = getDiagnosticsProviders(
				this.useLinterOnlyForSyntax,
				this.diagnosticsTimeoutMs,
				this.diagnosticsDelayMs,
				config.services.analyzer,
			)
			const preDiagnostics = (await Promise.all(providers.map((p) => p.capturePreSaveState()))).flat()

			// Apply changes
			const appliedResults: Array<{
				displayPath: string
				newProblemsMessage: string
			}> = []

			for (const fr of fileResults) {
				config.services.diffViewProvider.editType = "modify"
				await config.services.diffViewProvider.open(fr.absolutePath, { displayPath: fr.displayPath })
				await config.services.diffViewProvider.update(fr.finalContent, true)
				
				await setTimeoutPromise(200)
				
				const saveResult = await config.services.diffViewProvider.saveChanges({ skipDiagnostics: true })
				const actualFinalContent = saveResult.finalContent || fr.finalContent

				config.taskState.didEditFile = true
				config.services.fileContextTracker.markFileAsEditedByDirac(fr.displayPath)
				await config.services.fileContextTracker.trackFileContext(fr.displayPath, "dirac_edited")

				appliedResults.push({
					displayPath: fr.displayPath,
					newProblemsMessage: "",
				})

				// Update diagnostics for this file
				const diagnosticsData = [{
					filePath: fr.absolutePath,
					content: actualFinalContent,
				}]
				
				const providerDiagnostics = await Promise.all(
					providers.map((p) => p.getDiagnosticsFeedbackForFiles(diagnosticsData, preDiagnostics)),
				)

				for (const resultsOfProvider of providerDiagnostics) {
					const res = resultsOfProvider[0]
					if (res.newProblemsMessage) {
						appliedResults[appliedResults.length - 1].newProblemsMessage = res.newProblemsMessage
						break
					}
				}
			}

			config.taskState.consecutiveMistakeCount = 0
			telemetryService.captureToolUsage(
				config.ulid,
				this.name,
				config.api.getModel().id,
				provider,
				true,
				true,
				undefined,
				block.isNativeToolCall,
			)

			const summaries = appliedResults.map((ar) => {
				let summary = `Successfully renamed symbol in ${ar.displayPath}.`
				if (ar.newProblemsMessage) {
					summary += `\n\nNew problems detected after saving the file:\n${ar.newProblemsMessage}`
				}
				return summary
			})

			return `Successfully renamed symbol '${existingSymbol}' to '${newSymbol}' (${totalReplacements} occurrences in ${fileResults.length} files).\n\n${summaries.join("\n\n")}`

		} catch (error) {
			config.taskState.consecutiveMistakeCount++
			const errorMessage = error instanceof Error ? error.message : String(error)
			return formatResponse.formatToolErrorForLLM(createToolError("tool.internalError", `Error renaming symbol: ${errorMessage}`, "recoverable"))
		}
	}

	private getDiffBlock(
		originalLines: string[],
		originalHashes: string[],
		newLines: string[],
		startLine: number,
		endLine: number,
	): string {
		const contextBeforeCount = 3
		const contextAfterCount = 3

		const res: string[] = []
		const beforeStart = Math.max(0, startLine - contextBeforeCount)
		for (let i = beforeStart; i < startLine; i++) {
			res.push(` ${formatLineWithHash(originalLines[i], originalHashes[i])}`)
		}

		for (let i = startLine; i <= endLine; i++) {
			res.push(`-${originalLines[i]}`)
			res.push(`+${newLines[i]}`)
		}

		const afterEnd = Math.min(originalLines.length - 1, endLine + contextAfterCount)
		for (let i = endLine + 1; i <= afterEnd; i++) {
			res.push(` ${formatLineWithHash(originalLines[i], originalHashes[i])}`)
		}

		return res.join("\n")
	}
}
