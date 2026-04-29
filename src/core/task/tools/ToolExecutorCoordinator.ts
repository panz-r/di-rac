import type { ToolUse } from "@core/assistant-message"
import { formatResponse } from "@core/prompts/responses"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../index"
import { AskFollowupQuestionToolHandler } from "./handlers/AskFollowupQuestionToolHandler"
import { AttemptCompletionHandler } from "./handlers/AttemptCompletionHandler"
import { BrowserToolHandler } from "./handlers/BrowserToolHandler"
import { CondenseHandler } from "./handlers/CondenseHandler"
import { EditFileToolHandler } from "./handlers/EditFileToolHandler"
import { DiagnosticsScanToolHandler } from "./handlers/DiagnosticsScanToolHandler"
import { ExecuteCommandToolHandler } from "./handlers/ExecuteCommandToolHandler"
import { FindSymbolReferencesToolHandler } from "./handlers/FindSymbolReferencesToolHandler"
import { GenerateExplanationToolHandler } from "./handlers/GenerateExplanationToolHandler"
import { GetFileSkeletonToolHandler } from "./handlers/GetFileSkeletonToolHandler"
import { GetFunctionToolHandler } from "./handlers/GetFunctionToolHandler"
import { ListFilesToolHandler } from "./handlers/ListFilesToolHandler"
import { NewTaskHandler } from "./handlers/NewTaskHandler"
import { PlanModeRespondHandler } from "./handlers/PlanModeRespondHandler"
import { ReadFileToolHandler } from "./handlers/ReadFileToolHandler"
import { ReplaceSymbolToolHandler } from "./handlers/ReplaceSymbolToolHandler"
import { RenameSymbolToolHandler } from "./handlers/RenameSymbolToolHandler"
import { ReportBugHandler } from "./handlers/ReportBugHandler"
import { SearchFilesToolHandler } from "./handlers/SearchFilesToolHandler"
import { UseSubagentsToolHandler } from "./handlers/SubagentToolHandler"
import { SummarizeTaskHandler } from "./handlers/SummarizeTaskHandler"
import { UseSkillToolHandler } from "./handlers/UseSkillToolHandler"
import { ListSkillsToolHandler } from "./handlers/ListSkillsToolHandler"
import { WebFetchToolHandler } from "./handlers/WebFetchToolHandler"
import { WebSearchToolHandler } from "./handlers/WebSearchToolHandler"
import { ExpandSymbolToolHandler } from "./handlers/ExpandSymbolToolHandler"
import { SearchSymbolsToolHandler } from "./handlers/SearchSymbolsToolHandler"
import { RepoMapToolHandler } from "./handlers/RepoMapToolHandler"
import { BashToolHandler } from "./handlers/BashToolHandler"
import { CompactHandler } from "./handlers/CompactHandler"

import { WriteToFileToolHandler } from "./handlers/WriteToFileToolHandler"
import { AgentConfigLoader } from "./subagent/AgentConfigLoader"
import { ToolValidator } from "./ToolValidator"
import { TOOL_SCHEMAS } from "./schemas"
import { validateArgs } from "./validateArgs"
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
		[DiracDefaultTool.BASH]: (v: ToolValidator) => new ExecuteCommandToolHandler(v),
		[DiracDefaultTool.FILE_READ]: (v: ToolValidator) => new ReadFileToolHandler(v),
		[DiracDefaultTool.FILE_NEW]: (v: ToolValidator) => new WriteToFileToolHandler(v),
		[DiracDefaultTool.SEARCH]: (v: ToolValidator) => new SearchFilesToolHandler(v),
		[DiracDefaultTool.LIST_FILES]: (v: ToolValidator) => new ListFilesToolHandler(v),
		[DiracDefaultTool.GET_FUNCTION]: (v: ToolValidator) => new GetFunctionToolHandler(v),
		[DiracDefaultTool.GET_FILE_SKELETON]: (v: ToolValidator) => new GetFileSkeletonToolHandler(v),
		[DiracDefaultTool.FIND_SYMBOL_REFERENCES]: (v: ToolValidator) => new FindSymbolReferencesToolHandler(v),

		[DiracDefaultTool.EDIT_FILE]: (v: ToolValidator) => new EditFileToolHandler(v),
		[DiracDefaultTool.DIAGNOSTICS_SCAN]: (v: ToolValidator) => new DiagnosticsScanToolHandler(v),
		[DiracDefaultTool.REPLACE_SYMBOL]: (v: ToolValidator) => new ReplaceSymbolToolHandler(v),
		[DiracDefaultTool.RENAME_SYMBOL]: (v: ToolValidator) => new RenameSymbolToolHandler(v),
		[DiracDefaultTool.BROWSER]: (_v: ToolValidator) => new BrowserToolHandler(),

		[DiracDefaultTool.NEW_TASK]: (_v: ToolValidator) => new NewTaskHandler(),
		[DiracDefaultTool.PLAN_MODE]: (_v: ToolValidator) => new PlanModeRespondHandler(),
		[DiracDefaultTool.WEB_FETCH]: (_v: ToolValidator) => new WebFetchToolHandler(),
		[DiracDefaultTool.WEB_SEARCH]: (_v: ToolValidator) => new WebSearchToolHandler(),
		[DiracDefaultTool.CONDENSE]: (_v: ToolValidator) => new CondenseHandler(),
		[DiracDefaultTool.SUMMARIZE_TASK]: (_v: ToolValidator) => new SummarizeTaskHandler(_v),
		[DiracDefaultTool.REPORT_BUG]: (_v: ToolValidator) => new ReportBugHandler(),
		[DiracDefaultTool.NEW_RULE]: (v: ToolValidator) =>
			new SharedToolHandler(DiracDefaultTool.NEW_RULE, new WriteToFileToolHandler(v)),
		[DiracDefaultTool.GENERATE_EXPLANATION]: (_v: ToolValidator) => new GenerateExplanationToolHandler(),
		[DiracDefaultTool.USE_SKILL]: (_v: ToolValidator) => new UseSkillToolHandler(),
		[DiracDefaultTool.LIST_SKILLS]: (_v: ToolValidator) => new ListSkillsToolHandler(),
		[DiracDefaultTool.USE_SUBAGENTS]: (_v: ToolValidator) => new UseSubagentsToolHandler(),

		[DiracDefaultTool.EXPAND_SYMBOL]: (v: ToolValidator) => new ExpandSymbolToolHandler(v),
		[DiracDefaultTool.SEARCH_SYMBOLS]: (v: ToolValidator) => new SearchSymbolsToolHandler(v),
		[DiracDefaultTool.REPO_MAP]: (v: ToolValidator) => new RepoMapToolHandler(v),
		[DiracDefaultTool.COMPACT]: (v: ToolValidator) => new CompactHandler(v),
		[DiracDefaultTool.BASH_RESTRICTED]: (v: ToolValidator) => new BashToolHandler(v),
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

		// Pre-execution argument validation via Zod schema
		const schema = TOOL_SCHEMAS[block.name as DiracDefaultTool]
		if (schema) {
			const validation = validateArgs(schema, block.params, block.name)
			if (!validation.success) {
				return formatResponse.formatToolErrorForLLM(validation.error)
			}
		}

		return handler.execute(config, block)
	}
}
