import type { ToolUse } from "@core/assistant-message"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../index"
import { AskFollowupQuestionToolHandler } from "./handlers/AskFollowupQuestionToolHandler"
import { AttemptCompletionHandler } from "./handlers/AttemptCompletionHandler"
import { BrowserToolHandler } from "./handlers/BrowserToolHandler"
import { EditFileToolHandler } from "./handlers/EditFileToolHandler"
import { BashToolHandler } from "./handlers/BashToolHandler"
import { NewTaskHandler } from "./handlers/NewTaskHandler"
import { PlanModeRespondHandler } from "./handlers/PlanModeRespondHandler"
import { getToolHint } from "./utils/ToolHints"
import { computeAmbiguityScore } from "./utils/AmbiguityScorer"
import { ReadFileToolHandler } from "./handlers/ReadFileToolHandler"
import { SearchFilesToolHandler } from "./handlers/SearchFilesToolHandler"
import { UseSubagentsToolHandler } from "./handlers/SubagentToolHandler"
import { UseSkillToolHandler } from "./handlers/UseSkillToolHandler"
import { ListSkillsToolHandler } from "./handlers/ListSkillsToolHandler"
import { WebFetchToolHandler } from "./handlers/WebFetchToolHandler"
import { WebSearchToolHandler } from "./handlers/WebSearchToolHandler"
import { CompactHandler } from "./handlers/CompactHandler"
import { DiracUndoToolHandler } from "./handlers/DiracUndoToolHandler"
import { ToolSearchToolHandler } from "./handlers/ToolSearchToolHandler"
import { DiracOutputsToolHandler } from "./handlers/DiracOutputsToolHandler"
import { RecallHandler } from "./handlers/RecallHandler"
import { SymbolsToolHandler } from "./handlers/SymbolsToolHandler"
import { RepoToolHandler } from "./handlers/RepoToolHandler"
import { WriteToFileToolHandler } from "./handlers/WriteToFileToolHandler"
import { AgentConfigLoader } from "./subagent/AgentConfigLoader"
import { ToolValidator } from "./ToolValidator"
import { parseCliCommand, splitCommandChain, hasCliSchema, shouldChainSplit, getCliSchema } from "./cli-parser"
import type { TaskConfig } from "./types/TaskConfig"
import type { StronglyTypedUIHelpers } from "./types/UIHelpers"

export interface IToolHandler {
	readonly name: DiracDefaultTool
	execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse>
	getDescription(block: ToolUse): string
}

export interface IPartialBlockHandler {
	handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void>
}

export interface IFullyManagedTool extends IToolHandler, IPartialBlockHandler {
	// Marker interface for tools that handle their own complete approval flow
}

/**
 * A wrapper class that allows a single tool handler to be registered under multiple names.
 * This provides proper typing for tools that share the same implementation logic.
 */
export class SharedToolHandler implements IFullyManagedTool {
	constructor(
		public readonly name: DiracDefaultTool,
		private baseHandler: IFullyManagedTool,
	) {}

	getDescription(block: ToolUse): string {
		return this.baseHandler.getDescription(block)
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		return this.baseHandler.execute(config, block)
	}

	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		return this.baseHandler.handlePartialBlock(block, uiHelpers)
	}
}

/**
 * Coordinates tool execution by routing to registered handlers.
 * Falls back to legacy switch for unregistered tools.
 */
export class ToolExecutorCoordinator {
	private handlers = new Map<string, IToolHandler>()
	private dynamicSubagentHandlers = new Map<string, IToolHandler>()

	private readonly toolHandlersMap: Record<DiracDefaultTool, (v: ToolValidator) => IToolHandler | undefined> = {
		[DiracDefaultTool.ASK]: (_v: ToolValidator) => new AskFollowupQuestionToolHandler(),
		[DiracDefaultTool.ATTEMPT]: (_v: ToolValidator) => new AttemptCompletionHandler(),
		[DiracDefaultTool.BASH]: (v: ToolValidator) => new BashToolHandler(v),
		[DiracDefaultTool.FILE_READ]: (v: ToolValidator) => new ReadFileToolHandler(v),
		[DiracDefaultTool.FILE_NEW]: (v: ToolValidator) => new WriteToFileToolHandler(v),
		[DiracDefaultTool.EDIT_FILE]: (v: ToolValidator) => new EditFileToolHandler(v),
		[DiracDefaultTool.SYMBOLS]: (v: ToolValidator) => new SymbolsToolHandler(v),
		[DiracDefaultTool.SEARCH]: (v: ToolValidator) => new SearchFilesToolHandler(v),
		[DiracDefaultTool.LIST_FILES]: (v: ToolValidator) => new RepoToolHandler(v),
		[DiracDefaultTool.BROWSER]: (_v: ToolValidator) => new BrowserToolHandler(),
		[DiracDefaultTool.COMPACT]: (v: ToolValidator) => new CompactHandler(v),
		[DiracDefaultTool.NEW_TASK]: (_v: ToolValidator) => new NewTaskHandler(),
		[DiracDefaultTool.PLAN_MODE]: (_v: ToolValidator) => new PlanModeRespondHandler(),
		[DiracDefaultTool.WEB_FETCH]: (_v: ToolValidator) => new WebFetchToolHandler(),
		[DiracDefaultTool.WEB_SEARCH]: (_v: ToolValidator) => new WebSearchToolHandler(),
		[DiracDefaultTool.NEW_RULE]: (v: ToolValidator) =>
			new SharedToolHandler(DiracDefaultTool.NEW_RULE, new WriteToFileToolHandler(v)),
		[DiracDefaultTool.USE_SKILL]: (_v: ToolValidator) => new UseSkillToolHandler(),
		[DiracDefaultTool.LIST_SKILLS]: (_v: ToolValidator) => new ListSkillsToolHandler(),
		[DiracDefaultTool.USE_SUBAGENTS]: (_v: ToolValidator) => new UseSubagentsToolHandler(),
		[DiracDefaultTool.DIRAC_UNDO]: (_v: ToolValidator) => new DiracUndoToolHandler(),
		[DiracDefaultTool.TOOL_SEARCH]: (_v: ToolValidator) => new ToolSearchToolHandler(),
		[DiracDefaultTool.DIRAC_OUTPUTS]: (_v: ToolValidator) => new DiracOutputsToolHandler(),
		[DiracDefaultTool.DIRAC_RECALL]: (_v: ToolValidator) => new RecallHandler(),
	}

	/**
	 * Register a tool handler
	 */
	register(handler: IToolHandler): void {
		this.handlers.set(handler.name, handler)
	}

	registerByName(toolName: DiracDefaultTool, validator: ToolValidator): void {
		const handler = this.toolHandlersMap[toolName]?.(validator)
		if (handler) {
			this.register(handler)
		}
	}

	/**
	 * Check if a handler is registered for the given tool
	 */
	has(toolName: string): boolean {
		return this.getHandler(toolName) !== undefined
	}

	/**
	 * Get a handler for the given tool name
	 */
	getHandler(toolName: string): IToolHandler | undefined {
		const staticHandler = this.handlers.get(toolName)
		if (staticHandler) {
			return staticHandler
		}

		if (AgentConfigLoader.getInstance().isDynamicSubagentTool(toolName)) {
			const existingHandler = this.dynamicSubagentHandlers.get(toolName)
			if (existingHandler) {
				return existingHandler
			}
			const handler = new SharedToolHandler(toolName as DiracDefaultTool, new UseSubagentsToolHandler())
			this.dynamicSubagentHandlers.set(toolName, handler)
			return handler
		}

		return undefined
	}

	/**
	 * Execute a tool through its registered handler
	 */
	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const handler = this.getHandler(block.name)
		if (!handler) {
			throw new Error(`No handler registered for tool: ${block.name}`)
		}

		// Check if this is a CLI-migrated tool with a command string
		const commandStr = block.params?.command
		if (typeof commandStr === "string" && hasCliSchema(block.name)) {
			// Tools like execute_command handle shell operators internally — no chain splitting
			if (shouldChainSplit(block.name)) {
				const segments = splitCommandChain(commandStr)
				if (segments.length > 1) {
					return this.executeChain(config, block, handler, segments)
				}
			}
			const parsed = parseCliCommand(block.name, commandStr)
			if (parsed) block = { ...block, params: parsed }
			return this.executeSingle(config, block, handler)
		}

		// Fallback: when command is missing for a CLI tool, try to use any positional params
		// the LLM may have sent directly (e.g., { paths: ["."] } instead of { command: "." })
		if (hasCliSchema(block.name)) {
			const schema = getCliSchema(block.name)
			if (schema?.positionals) {
				const hasPositional = schema.positionals.some((pos) => {
					const val = (block.params as Record<string, unknown>)?.[pos.param]
					return val !== undefined && val !== null && val !== ""
				})
				if (hasPositional) {
					return this.executeSingle(config, block, handler)
				}
			}

			// Last resort: if block has any unrecognized string values that look like paths,
			// try to construct a command from them for the CLI parser
			const stringValues = Object.values(block.params || {}).filter(
				(v): v is string => typeof v === "string" && v.trim().length > 0
			)
			if (stringValues.length > 0) {
				const syntheticCommand = stringValues.join(" ")
				const parsed = parseCliCommand(block.name, syntheticCommand)
				if (parsed) {
					block = { ...block, params: parsed }
					return this.executeSingle(config, block, handler)
				}
			}
		}

		return this.executeSingle(config, block, handler)
	}

	private static CACHEABLE_TOOLS = new Set(["read", "search", "repo", "symbols"])

	private async executeSingle(
		config: TaskConfig,
		block: ToolUse,
		handler: IToolHandler,
	): Promise<ToolResponse> {
		// Cache lookup for read-only tools
		if (ToolExecutorCoordinator.CACHEABLE_TOOLS.has(block.name) && !block.params?.retry) {
			const cacheKey = `${block.name}:${ToolExecutorCoordinator.normalizeCacheArgs(block.params)}`
			const cached = config.taskState.toolResultCache.get(cacheKey)
			if (cached) return `[Cache Hit]${cached}`
		}
		// --dry-run: validate params without executing
		if (block.params?.dryRun) {
			const desc = handler.getDescription(block)
			return JSON.stringify({ status: "ok", data: null, hint: `[DRY RUN] Would execute: ${desc}`, meta: { tool: block.name, dryRun: true } })
		}


			// --clarify: detect ambiguity before executing
			if (block.params?.clarify) {
				const ambiguity = computeAmbiguityScore(block.name, block.params, config.taskState)
				if (ambiguity > 0.4) {
					const parsed = { ...block.params }
					delete parsed.clarify
					delete parsed.retry
					delete parsed.command
					if (ambiguity > 0.6) {
						return `[Clarify] Ambiguity: ${ambiguity.toFixed(2)} (High). P(success) < 0.3 — consider delegating to subagent or breaking task down. Parsed args: ${JSON.stringify(parsed)}`
					}
					return `[Clarify] Ambiguity: ${ambiguity.toFixed(2)} (Moderate). Parsed args: ${JSON.stringify(parsed)}`
				}
			}

		const maxRetries = block.params?.retry ? Math.min(Number(block.params.retry), 5) : 0
		let lastResult: ToolResponse = ""
		let attempts = 0

		while (attempts <= maxRetries) {
			const result = await handler.execute(config, block)
			lastResult = result

			// Only retry on string results that look like errors
			if (typeof result !== "string" || attempts >= maxRetries) break
			const trimmed = result.trimStart()
			if (!trimmed.startsWith("<tool_error")) break

			attempts++
			if (attempts <= maxRetries) {
				const delay = Math.min(500 * Math.pow(2, attempts - 1), 4000)
				await new Promise(r => setTimeout(r, delay))
			}
		}

		// Auto-correct truncated results
		if (typeof lastResult === "string" && block.params?.autoCorrect !== false) {
			const acResult = await this.autoCorrectIfTruncated(config, block, handler, lastResult)
			if (acResult !== null) lastResult = acResult
		}

		if (typeof lastResult === "string") {
			const hint = this.buildExplorationHint(block.name, block.params)
			let output = hint ? lastResult + hint : lastResult
			if (attempts > 0) {
				output = `[Retry] ${attempts} attempt${attempts > 1 ? "s" : ""}\n${output}`
			}
			// --clarify: prepend parsed params so LLM can verify interpretation
			if (block.params?.clarify) {
				const parsed = { ...block.params }
				delete parsed.clarify
				delete parsed.retry
				delete parsed.command
				output = `[Clarify] Parsed args: ${JSON.stringify(parsed)}\n${output}`
			}
			// --verify: re-read target file after write/edit
			if (block.params?.verify && !lastResult.trimStart().startsWith("<tool_error")) {
				const verifyPath = ToolExecutorCoordinator.extractTargetPath(block.name, block.params)
				if (verifyPath) {
					const readHandler = this.handlers.get("read")
					if (readHandler) {
						const verifyBlock = { ...block, params: { paths: [verifyPath], detail: "hint" } }
						const verifyResult = await readHandler.execute(config, verifyBlock as any)
						if (typeof verifyResult === "string") output += `\n[Verify] ${verifyResult}`
					}
				}
			}
			// Proactive ambiguity hint on non-clarify calls
			if (!block.params?.clarify) {
				const score = computeAmbiguityScore(block.name, block.params, config.taskState)
				if (score > 0.7) {
					output += `
[HINT] This call had high ambiguity (${score.toFixed(2)}). Consider using --clarify next time.`
				}
			}
			// Store in cache for read-only tools
			if (ToolExecutorCoordinator.CACHEABLE_TOOLS.has(block.name) && !output.startsWith("[Cache Hit]")) {
				const cacheKey = `${block.name}:${ToolExecutorCoordinator.normalizeCacheArgs(block.params)}`
				config.taskState.toolResultCache.set(cacheKey, lastResult)
			}
			return output
		}
		return lastResult
	}

	private async autoCorrectIfTruncated(
		config: TaskConfig,
		block: ToolUse,
		handler: IToolHandler,
		result: string,
	): Promise<string | null> {
		const degradationStrategies: Record<string, Record<string, any>> = {
			read: { detail: "skeleton" },
			search: { context_lines: 0 },
		}
		const strategy = degradationStrategies[block.name]
		if (!strategy) return null
		if (!result.includes("[truncated]") && !result.includes("... [Content reduced")) return null
		const degradedBlock = { ...block, params: { ...block.params, ...strategy } }
		try {
			const corrected = await handler.execute(config, degradedBlock)
			if (typeof corrected === "string") {
				return corrected + "\n[Auto-corrected: output was truncated, degraded params applied]"
			}
		} catch {
			// Fall through to original result
		}
		return null
	}

	private static extractTargetPath(tool: string, params: Record<string, any>): string | null {
		if (tool === "write") return params.path || null
		if (tool === "edit") return params.path || params.files?.[0]?.path || null
		return null
	}

	private static normalizeCacheArgs(params: Record<string, any>): string {
		const skipKeys = new Set(["clarify", "retry", "command", "call_id", "autoCorrect", "dryRun", "verify"])
		const clean: Record<string, any> = {}
		for (const key of Object.keys(params).sort()) {
			if (skipKeys.has(key)) continue
			let val = params[key]
			if (key === "path" && typeof val === "string") val = val.replace(/^\.\//, "")
			else if (key === "paths" && Array.isArray(val)) val = val.map((p: string) => p.replace(/^\.\//, ""))
			clean[key] = val
		}
		return JSON.stringify(clean)
	}



	private buildExplorationHint(tool: string, params: Record<string, any>): string | undefined {
		const hint = getToolHint(tool, "success", undefined, params)
		return hint ? "\n---\n" + hint : undefined
	}

	private async executeChain(
		config: TaskConfig,
		originalBlock: ToolUse,
		handler: IToolHandler,
		segments: { command: string; operator: string | null }[],
	): Promise<ToolResponse> {
		const results: string[] = []
		let lastSuccess = true

		for (let i = 0; i < segments.length; i++) {
			const seg = segments[i]
			const op = i === 0 ? null : (segments[i - 1]?.operator ?? null)

			// Conditional execution
			if (op === "&&" && !lastSuccess) break
			if (op === "||" && lastSuccess) break

			const parsed = parseCliCommand(originalBlock.name, seg.command)
			let block: ToolUse = { ...originalBlock, params: parsed ?? {} }

			const result = await handler.execute(config, block)
			if (typeof result === "string") {
				lastSuccess = true
				results.push(result)
			} else {
				lastSuccess = false
				results.push(JSON.stringify(result))
			}
		}

		return results.join("\n\n")
	}
}
