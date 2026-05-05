import path from "node:path"
import fs from "node:fs/promises"
import type { ToolUse } from "@core/assistant-message"
import { resolveWorkspacePath } from "@core/workspace"
import { AnalyzerClient } from "@/services/tree-sitter/AnalyzerClient"
import { contentHash, hashLines, formatLineWithHash } from "@utils/line-hashing"
import { getReadablePath, isLocatedInWorkspace } from "@utils/path"
import { formatResponse } from "@/core/prompts/responses"
import { telemetryService } from "@/services/telemetry"
import { DiracDefaultTool } from "@/shared/tools"
import { FileAnchorIndex } from "@/shared/utils/file-anchor-index"
import type { ToolResponse } from "../../index"
import { showNotificationForApproval } from "../../utils"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { ToolResultUtils } from "../utils/ToolResultUtils"
import { createToolError } from "@shared/tool-response"

export class ExpandSymbolToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.EXPAND_SYMBOL

	constructor(private validator: ToolValidator) {}

	getDescription(block: ToolUse): string {
		const relPath = block.params.path as string
		const symbolId = block.params.symbol as string
		return `${block.name} ${symbolId} in ${relPath}`
	}

	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		// Minimal partial handling
		const config = uiHelpers.getConfig()
		if (config.isSubagentExecution) return

		const partialMessage = JSON.stringify({
			tool: "expand_symbol",
			path: block.params.path,
			symbol: block.params.symbol,
		})

		await uiHelpers.say("tool", partialMessage, undefined, undefined, block.partial)
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const relPath = block.params.path as string
		const symbolId = block.params.symbol as string

		if (!relPath || !symbolId) {
			config.taskState.consecutiveMistakeCount++
			return await config.callbacks.sayAndCreateMissingParamError(this.name, !relPath ? "path" : "symbol")
		}

		const apiConfig = config.services.stateManager.getApiConfiguration()
		const currentMode = config.services.stateManager.getGlobalSettingsKey("mode")
		const provider = (currentMode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string

		try {
			// 1. Check diracignore
			const accessValidation = this.validator.checkDiracIgnorePath(relPath)
			if (!accessValidation.ok) {
				return formatResponse.diracIgnoreError(relPath)
			}

			// 2. Resolve path
			const pathResult = resolveWorkspacePath(config, relPath, "ExpandSymbolToolHandler.execute")
			const { absolutePath } = typeof pathResult === "string" ? { absolutePath: pathResult } : pathResult

			// 3. Find symbol range using tree-sitter daemon
			const fileContent = await fs.readFile(absolutePath, "utf8")
			const sourceLines = fileContent.split("\n")
			const daemonSymbols = await config.services.analyzer.outline(absolutePath)
			const definitions = AnalyzerClient.toParsedDefinitions(daemonSymbols, sourceLines)
			
			const target = definitions?.find(d => d.id === symbolId)
			if (!target || !target.fullBodyRange) {
				// Symbol not found or has no body - return outline to help LLM recover
				const outline = definitions ? 
					definitions.map(d => `  - [${d.id}] ${d.signature || d.text.trim()}`).join("\n") : 
					"No definitions found in file."
				
				return formatResponse.formatToolErrorForLLM(createToolError(
					"tool.internalError", 
					`Symbol '${symbolId}' not found or is a single-line definition in '${relPath}'. Available symbols:\n${outline}`, 
					"recoverable"
				))
			}

			// 4. Read symbol body (file content already loaded above)
			const lines = sourceLines
			const range = target.fullBodyRange
			
			const symbolLines = lines.slice(range.startLine, range.endLine + 1)
			const anchors = new FileAnchorIndex(lines).getAllHashes()
			const symbolAnchors = anchors.slice(range.startLine, range.endLine + 1)
			
			const formattedContent = symbolLines.map((line, i) => formatLineWithHash(line, symbolAnchors[i])).join("\n")
			const currentHash = contentHash(symbolLines.join("\n"))

			const result = `${relPath}::${symbolId}\n[Symbol Hash: ${currentHash}]\n${formattedContent}`

			// UI notification
			const completeMessage = JSON.stringify({
				tool: "expand_symbol",
				path: relPath,
				symbol: symbolId,
				operationIsLocatedInWorkspace: await isLocatedInWorkspace(relPath),
			})

			const shouldAutoApprove = config.isSubagentExecution || await config.callbacks.shouldAutoApproveToolWithPath(block.name, relPath)

			if (shouldAutoApprove) {
				if (!config.isSubagentExecution) {
					await config.callbacks.say("tool", completeMessage, undefined, undefined, false)
				}
				telemetryService.captureToolUsage(config.ulid, this.name, config.api.getModel().id, provider, true, true, undefined, block.isNativeToolCall)
			} else {
				await ToolResultUtils.askApprovalAndPushFeedback("tool", completeMessage, config)
				// Telemetry handled in askApproval...
			}

			await config.services.fileContextTracker.trackFileContext(relPath, "read_tool")
			config.taskState.consecutiveMistakeCount = 0
			return result

		} catch (error) {
			config.taskState.consecutiveMistakeCount++
			const errorMessage = error instanceof Error ? error.message : String(error)
			return formatResponse.formatToolErrorForLLM(createToolError("tool.internalError", `Error expanding symbol: ${errorMessage}`, "recoverable"))
		}
	}
}
