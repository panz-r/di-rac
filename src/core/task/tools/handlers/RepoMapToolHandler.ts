import path from "node:path"
import fs from "node:fs/promises"
import type { ToolUse } from "@core/assistant-message"
import { resolveWorkspacePath } from "@core/workspace"
import { AnalyzerClient } from "@/services/tree-sitter/AnalyzerClient"
import { getReadablePath, isLocatedInWorkspace } from "@utils/path"
import { readFirstNLines } from "@utils/fs"
import { formatResponse } from "@/core/prompts/responses"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { listFiles } from "@services/glob/list-files"
import { createToolError } from "@shared/tool-response"

const EXT_TO_LANG: Record<string, string> = {
	ts: "typescript", tsx: "typescript", js: "javascript", jsx: "javascript",
	py: "python", rs: "rust", go: "go", c: "c", cpp: "cpp",
	h: "c", hpp: "cpp", java: "java", php: "php", rb: "ruby",
	swift: "swift", kt: "java",
}

export class RepoMapToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.REPO_MAP
	private static readonly SUPPORTED_EXTENSIONS = new Set(["ts", "tsx", "js", "jsx", "py", "rs", "go", "c", "cpp", "h", "hpp", "java", "php", "rb", "swift", "kt"])

	constructor(private validator: ToolValidator) {}

	getDescription(block: ToolUse): string {
		return `[${block.name}]`
	}

	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		const config = uiHelpers.getConfig()
		if (config.isSubagentExecution) return
		await uiHelpers.say("tool", JSON.stringify({ tool: "repo_map" }), undefined, undefined, block.partial)
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const apiConfig = config.services.stateManager.getApiConfiguration()
		const currentMode = config.services.stateManager.getGlobalSettingsKey("mode")
		const provider = (currentMode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string

		try {
			// 1. Walk workspace
			const [fileInfos] = await listFiles(config.cwd, true, 500) // limit for repo map

			const results: any[] = []
			for (const file of fileInfos) {
				const absPath = file.path
				const relPath = path.relative(config.cwd, absPath)
				const ext = path.extname(absPath).toLowerCase().slice(1)

				const entry: any = {
					file: relPath,
					size: file.size,
				}

				if (RepoMapToolHandler.SUPPORTED_EXTENSIONS.has(ext)) {
					try {
						const previewContent = await readFirstNLines(absPath, 100)
						const lang = EXT_TO_LANG[ext] || ext
						const result = await config.services.analyzer.outlineContentWithImports(previewContent, lang)
						const sourceLines = previewContent.split("\n")
						const definitions = AnalyzerClient.toParsedDefinitions(result.symbols, sourceLines)
						const imports = result.imports

						// Capture top-level symbols
						entry.symbols = definitions
							.filter(s => s.kind === "class" || s.kind === "function" || s.kind === "interface")
							.map(s => s.name)
							.slice(0, 7)

						// Capture imports (rough heuristic for "edges")
						const workspaceImports = imports
							.filter(imp => imp.includes("./") || imp.includes("../") || imp.includes("@/"))
							.map(imp => imp.replace(/['";]/g, "").trim())
							.slice(0, 5)

						if (workspaceImports.length > 0) {
							entry.imports = workspaceImports
						}

						// Cache in symbol index if not already there
						if (!config.taskState.symbolIndex.has(relPath)) {
							config.taskState.symbolIndex.set(relPath, definitions.map(d => ({
								id: d.id,
								name: d.name,
								kind: d.kind,
								line: d.lineIndex + 1,
								signature: d.signature
							})))
						}
					} catch (e) {
						// Ignore parse errors
					}
				}
				results.push(entry)
			}

			// 2. Format concise output
			const output = results.map(r => {
				const syms = r.symbols && r.symbols.length > 0 ? ` [Symbols: ${r.symbols.join(", ")}]` : ""
				const imps = r.imports && r.imports.length > 0 ? ` [Imports: ${r.imports.join(", ")}]` : ""
				return `${r.file} (${Math.round(r.size / 1024)} KB)${syms}${imps}`
			}).join("\n")

			const completeMessage = JSON.stringify({
				tool: "repo_map",
				results: results,
			})

			if (!config.isSubagentExecution) {
				await config.callbacks.say("tool", completeMessage, undefined, undefined, false)
			}


			config.taskState.consecutiveMistakeCount = 0
			return `Repository Structure Summary:\n\n${output}\n\nUse read_file --detail outline to see full symbol tables for specific files.`

		} catch (error) {
			config.taskState.consecutiveMistakeCount++
			const errorMessage = error instanceof Error ? error.message : String(error)
			return formatResponse.formatToolErrorForLLM(createToolError("tool.internalError", `Error generating repo map: ${errorMessage}`, "recoverable"))
		}
	}
}
