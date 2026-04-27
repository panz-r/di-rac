import path from "node:path"
import fs from "node:fs/promises"
import type { ToolUse } from "@core/assistant-message"
import { resolveWorkspacePath } from "@core/workspace"
import { loadRequiredLanguageParsers } from "@services/tree-sitter/languageParser"
import { parseFile } from "@services/tree-sitter"
import { getReadablePath, isLocatedInWorkspace } from "@utils/path"
import { formatResponse } from "@/core/prompts/responses"
import { telemetryService } from "@/services/telemetry"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { ToolResultUtils } from "../utils/ToolResultUtils"
import { createToolError } from "@shared/tool-response"
import { listFiles } from "@services/glob/list-files"

export class SearchSymbolsToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.SEARCH_SYMBOLS
	private static readonly SUPPORTED_EXTENSIONS = new Set(["ts", "tsx", "js", "jsx", "py", "rs", "go", "c", "cpp", "h", "hpp", "java", "php", "rb", "swift", "kt"])

	constructor(private validator: ToolValidator) {}

	getDescription(block: ToolUse): string {
		const query = block.params.query as string
		const kind = block.params.kind as string || "all"
		return `[${block.name} for '${query}' (kind: ${kind})]`
	}

	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		const config = uiHelpers.getConfig()
		if (config.isSubagentExecution) return
		const partialMessage = JSON.stringify({
			tool: "search_symbols",
			query: block.params.query,
			kind: block.params.kind,
		})
		await uiHelpers.say("tool", partialMessage, undefined, undefined, block.partial)
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const query = (block.params.query as string || "").toLowerCase()
		const kindFilter = block.params.kind as string
		const maxResults = Number.parseInt(String(block.params.max_results || "20"))

		if (!query) {
			config.taskState.consecutiveMistakeCount++
			return await config.callbacks.sayAndCreateMissingParamError(this.name, "query")
		}

		const apiConfig = config.services.stateManager.getApiConfiguration()
		const currentMode = config.services.stateManager.getGlobalSettingsKey("mode")
		const provider = (currentMode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string

		try {
			// 1. Lazy Indexing if needed
			if (config.taskState.symbolIndex.size === 0) {
				const [fileInfos] = await listFiles(config.cwd, true, 1000) // limit to 1000 files for index
				const sourceFiles = fileInfos.filter(f => {
					const ext = path.extname(f.path).toLowerCase().slice(1)
					return SearchSymbolsToolHandler.SUPPORTED_EXTENSIONS.has(ext)
				})

				for (const file of sourceFiles) {
					const absPath = file.path
					const relPath = path.relative(config.cwd, absPath)
					try {
						const languageParsers = await loadRequiredLanguageParsers([absPath])
						const definitions = await parseFile(absPath, languageParsers, config.services.diracIgnoreController)
						if (definitions) {
							config.taskState.symbolIndex.set(relPath, definitions.map(d => ({
								id: d.id,
								name: d.name,
								kind: d.kind,
								line: d.lineIndex + 1,
								signature: d.signature
							})))
						}
					} catch (e) {
						// Skip files that fail to parse
					}
				}
			}

			// 2. Search the index
			const matches: any[] = []
			outer: for (const [relPath, symbols] of config.taskState.symbolIndex.entries()) {
				for (const sym of symbols) {
					if (sym.name.toLowerCase().includes(query) || sym.id.toLowerCase().includes(query)) {
						if (!kindFilter || sym.kind === kindFilter || (kindFilter === "function" && sym.kind === "method")) {
							matches.push({ ...sym, file: relPath })
						}
					}
					if (matches.length >= maxResults) break outer
				}
			}

			// 3. Format result
			let resultText = ""
			if (matches.length === 0) {
				resultText = `No symbols matching '${query}' found.`
			} else {
				resultText = `Found ${matches.length} matching symbols:\n` + matches.map(m => 
					`  - [${m.id}] ${m.signature || m.name} in '${m.file}' (line ${m.line})`
				).join("\n")
			}

			const completeMessage = JSON.stringify({
				tool: "search_symbols",
				query,
				kind: kindFilter,
				results: matches,
			})

			if (config.isSubagentExecution) {
				telemetryService.captureToolUsage(config.ulid, this.name, config.api.getModel().id, provider, true, true, undefined, block.isNativeToolCall)
			} else {
				await config.callbacks.say("tool", completeMessage, undefined, undefined, false)
				telemetryService.captureToolUsage(config.ulid, this.name, config.api.getModel().id, provider, true, true, undefined, block.isNativeToolCall)
			}

			config.taskState.consecutiveMistakeCount = 0
			return resultText

		} catch (error) {
			config.taskState.consecutiveMistakeCount++
			const errorMessage = error instanceof Error ? error.stack || error.message : String(error)
			return formatResponse.formatToolErrorForLLM(createToolError("tool.internalError", `Error searching symbols: ${errorMessage}`, "recoverable"))
		}
	}
}
