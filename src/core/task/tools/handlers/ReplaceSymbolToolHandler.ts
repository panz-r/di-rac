// tool call test comment
import { ToolUse } from "@core/assistant-message"
import { setTimeout as setTimeoutPromise } from "node:timers/promises"
import { resolveWorkspacePath } from "@core/workspace"
import { FileAnchorIndex } from "@shared/utils/file-anchor-index"
import { ASTAnchorBridge, SymbolRange } from "@utils/ASTAnchorBridge"
import { formatLineWithHash, stripHashes } from "@utils/line-hashing"
import { getReadablePath } from "@utils/path"
import * as fs from "fs/promises"
import * as path from "path"
import { formatResponse } from "@/core/prompts/responses"
import { createToolError } from "@shared/tool-response"
import { HostProvider } from "@/hosts/host-provider"
import { getDiagnosticsProviders } from "@/integrations/diagnostics/getDiagnosticsProviders"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import { showNotificationForApproval } from "../../utils"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { ToolResultUtils } from "../utils/ToolResultUtils"

interface Replacement {
	path: string
	symbol: string
	text: string
	type?: string
}

interface FileBatch {
	absolutePath: string
	displayPath: string
	replacements: Replacement[]
}

export class ReplaceSymbolToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.REPLACE_SYMBOL
	public diagnosticsTimeoutMs = 1500
	public diagnosticsDelayMs = 500

	constructor(
		private validator: ToolValidator,
		private readonly useLinterOnlyForSyntax: boolean = false,
	) {}

	getDescription(block: ToolUse): string {
		const replacements = Array.isArray(block.params.replacements) ? block.params.replacements : []
		if (replacements.length > 0) {
			const symbols = replacements.map((r) => r.symbol).join(", ")
			const paths = Array.from(new Set(replacements.map((r) => r.path))).join(", ")
			return `${block.name} symbols ${symbols} in ${paths}`
		}
		return `${block.name} ${block.params.symbol} in ${block.params.path}`
	}

	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		const replacements = Array.isArray(block.params.replacements) ? block.params.replacements : []
		const config = uiHelpers.getConfig()
		if (config.isSubagentExecution) {
			return
		}

		// Fallback for old format if AI still uses it during transition
		const relPath = block.params.path || (replacements.length > 0 ? replacements[0].path : "")
		const symbol =
			block.params.symbol || (replacements.length > 1 ? `${replacements.length} symbols` : replacements[0]?.symbol || "")

		const sharedMessageProps = {
			tool: "replaceSymbol",
			path: getReadablePath(config.cwd, uiHelpers.removeClosingTag(block, "path", relPath)),
			symbol: uiHelpers.removeClosingTag(block, "symbol", symbol),
			replacementsCount: replacements.length,
		}

		const partialMessage = JSON.stringify(sharedMessageProps)

		if (await uiHelpers.shouldAutoApproveToolWithPath(block.name, relPath)) {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("ask", "tool")
			await uiHelpers.say("tool", partialMessage, undefined, undefined, block.partial)
		} else {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("say", "tool")
			await uiHelpers.ask("tool", partialMessage, block.partial).catch(() => {})
		}
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const replacements: Replacement[] = Array.isArray(block.params.replacements)
			? block.params.replacements
			: block.params.path && block.params.symbol && block.params.text
				? [
						{
							path: block.params.path,
							symbol: block.params.symbol,
							text: block.params.text,
							type: block.params.type,
						},
					]
				: []

		if (replacements.length === 0) {
			config.taskState.consecutiveMistakeCount++
			return await config.callbacks.sayAndCreateMissingParamError(this.name, "replacements")
		}

		const apiConfig = config.services.stateManager.getApiConfiguration()
		const currentMode = config.services.stateManager.getGlobalSettingsKey("mode")
		const provider = (currentMode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string

		try {
			// Group replacements by file
			const batches = new Map<string, FileBatch>()
			for (const r of replacements) {
				const pathResult = resolveWorkspacePath(config, r.path, "ReplaceSymbolToolHandler.execute")
				const { absolutePath, displayPath } =
					typeof pathResult === "string" ? { absolutePath: pathResult, displayPath: r.path } : pathResult

				if (!batches.has(absolutePath)) {
					batches.set(absolutePath, { absolutePath, displayPath, replacements: [] })
				}
				batches.get(absolutePath)!.replacements.push(r)
			}

			const fileResults: Array<{
				batch: FileBatch
				originalContent: string
				finalContent: string
				diff: string
				individualDiffs: string[]
				replacementsWithDiffs: Array<Replacement & { diff: string }>
			}> = []

			for (const batch of batches.values()) {
				await HostProvider.workspace.saveOpenDocumentIfDirty({ filePath: batch.absolutePath })
				const originalContent = await fs.readFile(batch.absolutePath, "utf8")
				const originalLines = originalContent.split(/\r?\n/)
				const originalHashes = new FileAnchorIndex(originalLines).getAllHashes()

				const resolvedReplacements: Array<{
					replacement: Replacement
					range: SymbolRange
				}> = []

				for (const r of batch.replacements) {
					const symbolRange = await ASTAnchorBridge.getSymbolRange(
						batch.absolutePath,
						r.symbol,
						r.type,
						config.services.diracIgnoreController,
						config.ulid,
						config.services.analyzer,
					)

					if (!symbolRange) {
						return formatResponse.formatToolErrorForLLM(createToolError("anchor.notFound", `Symbol '${r.symbol}'${r.type ? ` of type '${r.type}'` : ""} not found in ${r.path}.`, "recoverable"))
					}
					resolvedReplacements.push({ replacement: r, range: symbolRange })
				}

				// Check for overlaps
				resolvedReplacements.sort((a, b) => a.range.startIndex - b.range.startIndex)
				for (let i = 0; i < resolvedReplacements.length - 1; i++) {
					if (resolvedReplacements[i].range.endIndex > resolvedReplacements[i + 1].range.startIndex) {
						return formatResponse.formatToolErrorForLLM(createToolError("edit.multiFileConflict", `Overlapping replacements detected for symbols '${resolvedReplacements[i].replacement.symbol}' and '${resolvedReplacements[i + 1].replacement.symbol}' in ${batch.displayPath}.`, "unrecoverable"))
					}
				}

				// Apply replacements from bottom to top
				let currentContent = originalContent
				const individualDiffs: string[] = []
				const replacementsWithDiffs: Array<Replacement & { diff: string }> = []

				// Sort DESC for application
				const sortedForApplication = [...resolvedReplacements].sort((a, b) => b.range.startIndex - a.range.startIndex)

				for (const { replacement, range } of sortedForApplication) {
					const newText = stripHashes(replacement.text)

					// Strip the leading whitespace 
					const lineStart = currentContent.lastIndexOf("\n", range.startIndex - 1) + 1
					const leadingWhitespaceBefore = currentContent.slice(lineStart, range.startIndex)
					const adjustedNewText =
						leadingWhitespaceBefore.length > 0 && /^[ \t]+$/.test(leadingWhitespaceBefore)
							? newText.replace(/^[ \t]*/, (match) => match.slice(leadingWhitespaceBefore.length))
							: newText

					currentContent = currentContent.slice(0, range.startIndex) + adjustedNewText + currentContent.slice(range.endIndex)

					const diff = this.getDiffBlock(
						originalLines,
						originalHashes,
						originalContent,
						newText,
						range.startIndex,
						range.endIndex,
						range.startLine,
					)
					individualDiffs.push(diff)
					replacementsWithDiffs.push({ ...replacement, diff })
				}

				fileResults.push({
					batch,
					originalContent,
					finalContent: currentContent,
					diff: individualDiffs.reverse().join("\n\n---\n\n"),
					individualDiffs,
					replacementsWithDiffs: replacementsWithDiffs.reverse(),
				})
			}

			// Handle approval
			const allDiffs = fileResults.map((fr) => `*** Update File: ${fr.batch.displayPath}\n\n${fr.diff}`).join("\n\n")
			const allReplacements = fileResults.flatMap((fr) => fr.replacementsWithDiffs)

			const completeMessage = JSON.stringify({
				tool: "replaceSymbol",
				path: fileResults.length === 1 ? fileResults[0].batch.displayPath : "Multiple files",
				symbol: replacements.length === 1 ? replacements[0].symbol : `${replacements.length} symbols`,
				diff: allDiffs,
				replacements: allReplacements,
			})

			const shouldAutoApprove =
				config.isSubagentExecution ||
				(await Promise.all(
					Array.from(batches.values()).map((b) => config.callbacks.shouldAutoApproveToolWithPath(this.name, b.displayPath)),
				).then((results) => results.every(Boolean)))

			if (!shouldAutoApprove) {
				const symbolNames = replacements.map((r) => r.symbol).join(", ")
				const fileNames = Array.from(batches.values())
					.map((b) => path.basename(b.absolutePath))
					.join(", ")
				const notificationMessage = `di wants to replace symbols [${symbolNames}] in [${fileNames}]`
				showNotificationForApproval(notificationMessage, config.autoApprovalSettings.enableNotifications)

				await config.callbacks.removeLastPartialMessageIfExistsWithType("say", "tool")
				await config.callbacks.removeLastPartialMessageIfExistsWithType("ask", "tool")
				const { didApprove } = await ToolResultUtils.askApprovalAndPushFeedback("tool", completeMessage, config)
				if (!didApprove) {
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
				batch: FileBatch
				finalContent: string
				newProblemsMessage: string
			}> = []

			for (const fr of fileResults) {
				const { batch, finalContent } = fr

				// Apply changes via DiffViewProvider
				config.services.diffViewProvider.editType = "modify"
				await config.services.diffViewProvider.open(batch.absolutePath, { displayPath: batch.displayPath })
				await config.services.diffViewProvider.update(finalContent, true)

				// Wait for the diff view to update before saving to ensure auto-formatting is triggered
				await setTimeoutPromise(200)

				// Save with skipDiagnostics: true because we'll run them in parallel at the end
				const saveResult = await config.services.diffViewProvider.saveChanges({ skipDiagnostics: true })
				const actualFinalContent = saveResult.finalContent || finalContent

				config.taskState.consecutiveMistakeCount = 0
				config.taskState.didEditFile = true
				config.services.fileContextTracker.markFileAsEditedByDirac(batch.displayPath)
				await config.services.fileContextTracker.trackFileContext(batch.displayPath, "dirac_edited")

				appliedResults.push({
					batch,
					finalContent: actualFinalContent,
					newProblemsMessage: "", // Will be populated below
				})
			}

			// Run diagnostics in parallel for all files
			if (appliedResults.length > 0) {
				const diagnosticsData = appliedResults.map((ar) => ({
					filePath: ar.batch.absolutePath,
					content: ar.finalContent,
				}))

				const providerDiagnostics = await Promise.all(
					providers.map((p) => p.getDiagnosticsFeedbackForFiles(diagnosticsData, preDiagnostics)),
				)

				// Combine results
				for (let i = 0; i < appliedResults.length; i++) {
					const ar = appliedResults[i]
					for (const resultsOfProvider of providerDiagnostics) {
						const res = resultsOfProvider[i]
						if (res.newProblemsMessage) {
							ar.newProblemsMessage = res.newProblemsMessage
							break
						}
					}
				}
			}


			const summaries = appliedResults.map((ar) => {
				const symbolList = ar.batch.replacements.map((r) => `'${r.symbol}'`).join(", ")
				let summary = `Successfully replaced symbols ${symbolList} in ${ar.batch.displayPath}. Any existing hash anchors for these symbols are now stale.`
				if (ar.newProblemsMessage) {
					summary += `\n\nNew problems detected after saving the file:\n${ar.newProblemsMessage}`
				}
				return summary
			})

			return summaries.join("\n\n")
		} catch (error) {
			config.taskState.consecutiveMistakeCount++
			const errorMessage = error instanceof Error ? error.message : String(error)
			return formatResponse.formatToolErrorForLLM(createToolError("tool.internalError", `Error replacing symbols: ${errorMessage}`, "recoverable"))
		}
	}

	private getDiffBlock(
		originalLines: string[],
		originalHashes: string[],
		originalContent: string,
		newText: string,
		startIndex: number,
		endIndex: number,
		startLine: number,
	): string {
		const contextBeforeCount = 3
		const contextAfterCount = 3

		const originalTextInRange = originalContent.slice(startIndex, endIndex)
		const originalLinesInRange = originalTextInRange.split(/\r?\n/)
		const newLinesInRange = newText.split(/\r?\n/)

		const res: string[] = []
		const beforeStart = Math.max(0, startLine - contextBeforeCount)
		for (let i = beforeStart; i < startLine; i++) {
			res.push(` ${formatLineWithHash(originalLines[i], originalHashes[i])}`)
		}

		for (const line of originalLinesInRange) {
			res.push(`-${line}`)
		}
		for (const line of newLinesInRange) {
			res.push(`+${line}`)
		}

		const endLineOriginal = startLine + originalLinesInRange.length
		const afterEnd = Math.min(originalLines.length - 1, endLineOriginal + contextAfterCount)
		for (let i = endLineOriginal; i <= afterEnd; i++) {
			res.push(` ${formatLineWithHash(originalLines[i], originalHashes[i])}`)
		}

		return res.join("\n")
	}
}
