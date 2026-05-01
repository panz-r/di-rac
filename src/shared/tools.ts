import { Tool as AnthropicTool } from "@anthropic-ai/sdk/resources/index"
import { FunctionDeclaration as GoogleTool } from "@google/genai"
import { ChatCompletionTool as OpenAITool } from "openai/resources/chat/completions"

export type DiracTool = OpenAITool | AnthropicTool | GoogleTool

// Define available tool ids
export enum DiracDefaultTool {
	ASK = "ask_followup_question",
	ATTEMPT = "attempt_completion",
	BASH = "execute_command",
	BASH_RESTRICTED = "bash",
	FILE_READ = "read_file",
	FILE_NEW = "write_to_file",
	SEARCH = "search_files",
	LIST_FILES = "list_files",
	BROWSER = "browser_action",
	NEW_TASK = "new_task",
	PLAN_MODE = "plan_mode_respond",
	WEB_FETCH = "web_fetch",
	WEB_SEARCH = "web_search",
	CONDENSE = "condense",
	SUMMARIZE_TASK = "summarize_task",
	REPORT_BUG = "report_bug",
	NEW_RULE = "new_rule",
	USE_SKILL = "use_skill",
	LIST_SKILLS = "list_skills",
	USE_SUBAGENTS = "use_subagents",
	GET_FUNCTION = "get_function",
	GET_FILE_SKELETON = "get_file_skeleton",
	FIND_SYMBOL_REFERENCES = "find_symbol_references",

	EDIT_FILE = "edit_file",
	DIAGNOSTICS_SCAN = "diagnostics_scan",
	REPLACE_SYMBOL = "replace_symbol",
	RENAME_SYMBOL = "rename_symbol",

	EXPAND_SYMBOL = "expand_symbol",
	SEARCH_SYMBOLS = "search_symbols",
	REPO_MAP = "repo_map",
	COMPACT = "compact",
	DIRAC_UNDO = "dirac_undo",
}

// Array of all tool names for compatibility
// Automatically generated from the enum values
export const toolUseNames = Object.values(DiracDefaultTool) as DiracDefaultTool[]

const dynamicToolUseNamesByNamespace = new Map<string, Set<string>>()

export function setDynamicToolUseNames(namespace: string, names: string[]): void {
	dynamicToolUseNamesByNamespace.set(namespace, new Set(names.map((name) => name.trim()).filter(Boolean)))
}

export function getToolUseNames(): string[] {
	const defaults = [...toolUseNames]
	const dynamic = Array.from(dynamicToolUseNamesByNamespace.values()).flatMap((set) => Array.from(set))
	return Array.from(new Set([...defaults, ...dynamic]))
}

// Tools that do not modify the workspace state
export const READ_ONLY_TOOLS = [
	DiracDefaultTool.LIST_FILES,
	DiracDefaultTool.FILE_READ,
	DiracDefaultTool.SEARCH,
	DiracDefaultTool.BROWSER,
	DiracDefaultTool.ASK,
	DiracDefaultTool.GET_FUNCTION,
	DiracDefaultTool.GET_FILE_SKELETON,
	DiracDefaultTool.FIND_SYMBOL_REFERENCES,
	DiracDefaultTool.DIAGNOSTICS_SCAN,

	DiracDefaultTool.WEB_SEARCH,
	DiracDefaultTool.WEB_FETCH,
	DiracDefaultTool.USE_SKILL,
	DiracDefaultTool.LIST_SKILLS,
	DiracDefaultTool.USE_SUBAGENTS,

	DiracDefaultTool.EXPAND_SYMBOL,
	DiracDefaultTool.SEARCH_SYMBOLS,
	DiracDefaultTool.REPO_MAP,
	DiracDefaultTool.BASH_RESTRICTED,
] as const
