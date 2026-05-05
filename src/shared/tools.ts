import { Tool as AnthropicTool } from "@anthropic-ai/sdk/resources/index"
import { FunctionDeclaration as GoogleTool } from "@google/genai"
import { ChatCompletionTool as OpenAITool } from "openai/resources/chat/completions"

export type DiracTool = OpenAITool | AnthropicTool | GoogleTool

export enum DiracDefaultTool {
	ASK = "ask",
	ATTEMPT = "done",
	BASH = "bash",
	FILE_READ = "read",
	FILE_NEW = "write",
	SEARCH = "search",
	LIST_FILES = "repo",
	EDIT_FILE = "edit",
	SYMBOLS = "symbols",
	COMPACT = "compact",
	NEW_TASK = "task",
	PLAN_MODE = "plan",
	BROWSER = "browser_action",
	USE_SKILL = "use_skill",
	LIST_SKILLS = "list_skills",
	USE_SUBAGENTS = "use_subagents",
	WEB_FETCH = "web_fetch",
	WEB_SEARCH = "web_search",
	TOOL_SEARCH = "tools",
	DIRAC_OUTPUTS = "memory",
	DIRAC_RECALL = "recall",
	NEW_RULE = "new_rule",
	DIRAC_UNDO = "dirac_undo",
}

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

export const READ_ONLY_TOOLS = [
	DiracDefaultTool.LIST_FILES,
	DiracDefaultTool.FILE_READ,
	DiracDefaultTool.SEARCH,
	DiracDefaultTool.BROWSER,
	DiracDefaultTool.ASK,
	DiracDefaultTool.USE_SKILL,
	DiracDefaultTool.LIST_SKILLS,
	DiracDefaultTool.USE_SUBAGENTS,
	DiracDefaultTool.TOOL_SEARCH,
	DiracDefaultTool.WEB_SEARCH,
	DiracDefaultTool.WEB_FETCH,
	DiracDefaultTool.SYMBOLS,
	DiracDefaultTool.DIRAC_RECALL,
	DiracDefaultTool.BASH,
] as const
