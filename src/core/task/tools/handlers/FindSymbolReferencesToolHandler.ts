import { ToolUse } from "@core/assistant-message"
import { resolveWorkspacePath } from "@core/workspace"
import { FileAnchorIndex } from "@shared/utils/file-anchor-index"
import { formatLineWithHash, stripHashes } from "@utils/line-hashing"
import { getReadablePath, isLocatedInWorkspace } from "@utils/path"
import { createToolError } from "@shared/tool-response"
import * as fs from "fs/promises"
import * as path from "path"
import { formatResponse } from "@/core/prompts/responses"
import { SymbolIndexService } from "@/services/symbol-index/SymbolIndexService"
import { telemetryService } from "@/services/telemetry"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import { showNotificationForApproval } from "../../utils"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { ToolResultUtils } from "../utils/ToolResultUtils"

export class FindSymbolReferencesToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.FIND_SYMBOL_REFERENCES

	constructor(private validator: ToolValidator) {}

	getDescription(block: ToolUse): string {
		const relPaths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : (block.params.path ? [block.params.path as string] : []))
		const symbols = Array.isArray(block.params.symbols) ? block.params.symbols : (block.params.symbols ? [block.params.symbols as string] : (block.params.symbol ? [block.params.symbol as string] : []))
		const findType = (block.params.find_type as string) || "both"
		return `${block.name} ${symbols.join(", ")} in ${relPaths.join(" ")}${findType !== "both" ? ` (type: ${findType})` : ""}`
	}

	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		const relPaths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : (block.params.path ? [block.params.path as string] : []))
		const symbols = Array.isArray(block.params.symbols) ? block.params.symbols : (block.params.symbols ? [block.params.symbols as string] : (block.params.symbol ? [block.params.symbol as string] : []))

		const config = uiHelpers.getConfig()
		if (config.isSubagentExecution) {
			return
		}

		const firstPath = relPaths[0] || ""
		const sharedMessageProps = {
			tool: "findSymbolReferences",
			path: getReadablePath(config.cwd, uiHelpers.removeClosingTag(block, "paths", firstPath)),
			paths: relPaths.map((p) => getReadablePath(config.cwd, p)),
			symbol: uiHelpers.removeClosingTag(block, "symbols", symbols[0] || ""),
			symbols,
			find_type: uiHelpers.removeClosingTag(block, "find_type", (block.params.find_type as string) || "both"),
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
		const symbols = Array.isArray(block.params.symbols) ? block.params.symbols : (block.params.symbols ? [block.params.symbols as string] : (block.params.symbol ? [block.params.symbol as string] : []))
		const findType = (block.params.find_type as "definition" | "reference" | "both") || "both"

		const apiConfig = config.services.stateManager.getApiConfiguration()
		const currentMode = config.services.stateManager.getGlobalSettingsKey("mode")
		const provider = (currentMode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string

		if (relPaths.length === 0 || symbols.length === 0) {
			config.taskState.consecutiveMistakeCount++
			return await config.callbacks.sayAndCreateMissingParamError(this.name, relPaths.length === 0 ? "paths" : "symbols")
		}

		let result = ""
		const references: { path: string; refs: string[] }[] = []
		try {
			const indexService = SymbolIndexService.getInstance()
			const projectRoot = config.workspaceManager?.getPrimaryRoot()?.path || config.cwd
			const absolutePaths = relPaths.map((p) => {
				const pathResult = resolveWorkspacePath(config, p, "FindSymbolReferencesToolHandler.execute")
				const absPath = typeof pathResult === "string" ? pathResult : pathResult.absolutePath
				return path.resolve(absPath)
			})

			// Ensure the index is initialized for the current project root
			if (indexService.getProjectRoot() !== projectRoot) {
				await indexService.initialize(projectRoot)
			}

			// Synchronously index requested files if they are not yet in the index.
			if (absolutePaths.length <= 100) {
				for (const absPath of absolutePaths) {
					try {
						const stats = await fs.stat(absPath)
						if (stats.isFile()) {
							await indexService.updateFile(absPath)
						}
					} catch (e) {
						// Skip if file doesn't exist or other error
					}
				}
			}

			// Group symbols by file across the entire project
			const fileHitsMap = new Map<
				string,
				{
					symbol: string
					startLine: number
					startColumn: number
					endLine: number
					endColumn: number
					type: "definition" | "reference"
				}[]
			>()

			for (const symbol of symbols) {
				let locations: any[] = []
				if (findType === "definition") {
					locations = await indexService.searchSymbolsDaemon(symbol, "definition")
				} else if (findType === "reference") {
					locations = await indexService.searchSymbolsDaemon(symbol, "reference")
				} else {
					locations = await indexService.searchSymbolsDaemon(symbol)
				}

				for (const loc of locations) {
					const absLocPath = path.join(projectRoot, loc.path)

					// Check if this location is within one of the requested paths
					const isInRequestedPath = absolutePaths.some(
						(requestedPath) => absLocPath === requestedPath || absLocPath.startsWith(requestedPath + path.sep),
					)

					if (isInRequestedPath) {
						let hits = fileHitsMap.get(absLocPath)
						if (!hits) {
							hits = []
							fileHitsMap.set(absLocPath, hits)
						}
						hits.push({
							symbol,
							startLine: loc.startLine,
							startColumn: loc.startColumn,
							endLine: loc.endLine,
							endColumn: loc.endColumn,
							type: loc.type,
						})
					}
				}
			}

			if (fileHitsMap.size === 0) {
				result = `No ${findType === "both" ? "references or definitions" : findType + "s"} found for symbols: ${symbols.join(", ")}.`
			} else {
				let output = ""
				// Sort files by path for deterministic output
				const sortedFiles = Array.from(fileHitsMap.keys()).sort()

				for (const absFilePath of sortedFiles) {
					try {
						const fileHits = fileHitsMap.get(absFilePath)!
						const fileContent = await fs.readFile(absFilePath, "utf8")
						const lines = fileContent.split(/\r?\n/)
						const anchors = new FileAnchorIndex(lines).getAllHashes()

						// Sort and merge hits on the same line
						const sortedHits = [...fileHits].sort((a, b) => a.startLine - b.startLine)
						const mergedHits: { startLine: number; symbols: Set<string> }[] = []

						for (const hit of sortedHits) {
							const last = mergedHits[mergedHits.length - 1]
							if (last && last.startLine === hit.startLine) {
								last.symbols.add(hit.symbol)
							} else {
								mergedHits.push({
									startLine: hit.startLine,
									symbols: new Set([hit.symbol]),
								})
							}
						}

						const fileRefs: string[] = []
						for (const hit of mergedHits) {
							const hitSymbols = Array.from(hit.symbols).join(", ")
							const lineContent = lines[hit.startLine]
							const formattedLine = formatLineWithHash(lineContent, anchors[hit.startLine])
							fileRefs.push(`  (${hitSymbols}) ${formattedLine.trim()}`)
						}

						const relPath = path.relative(config.cwd, absFilePath)
						output += `${relPath}:\n${fileRefs.join("\n")}\n\n`
						references.push({ path: relPath, refs: fileRefs })
					} catch (error) {
						output += `Error reading file ${absFilePath}: ${error}\n`
					}
				}
				result = output.trim()
			}
		} catch (error) {
			config.taskState.consecutiveMistakeCount++
			const errorMessage = error instanceof Error ? error.message : String(error)
			return formatResponse.formatToolErrorForLLM(createToolError("tool.internalError", `Error finding references: ${errorMessage}`, "recoverable"))
		}

		if (
			result.includes("No references found") ||
			result.includes("No definitions found") ||
			result.includes("No references or definitions found")
		) {
			config.taskState.consecutiveMistakeCount++
		} else {
			config.taskState.consecutiveMistakeCount = 0
		}

		const sharedMessageProps = {
			tool: "findSymbolReferences",
			paths: relPaths.map((p) => getReadablePath(config.cwd, p)),
			symbols,
			references: references.map((r) => ({
				path: r.path,
				refs: r.refs.map((ref) => stripHashes(ref)),
			})),
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
			const notificationMessage = `di wants to find ${findType === "both" ? "references" : findType + "s"} for ${symbols.length} symbol(s) in ${relPaths.length} path(s)`
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
