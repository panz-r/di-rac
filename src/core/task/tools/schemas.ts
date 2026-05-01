/**
 * Zod schemas for all tool argument validations.
 *
 * Each schema corresponds to a tool's expected `block.params` shape
 * and can be attached to tool definitions for use by the dispatcher's
 * pre-execution validation layer.
 *
 * IMPORTANT: These schemas validate the RAW params as received from the LLM.
 * Many LLM providers stringify arrays/objects inside parameters, so schemas
 * include preprocessors to handle JSON-string coercion.
 */
import { z } from "zod";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Coerces a value that might be a JSON-stringified array into a real array.
 * Handles the common LLM artefact where arrays arrive as strings.
 */
const stringToArray = <T extends z.ZodTypeAny>(schema: T) =>
	z.preprocess((val) => {
		if (typeof val === "string") {
			try {
				const parsed = JSON.parse(val);
				if (Array.isArray(parsed)) return parsed;
			} catch {
				// not valid JSON, return as-is for schema to reject
			}
		}
		return val;
	}, z.array(schema));

/** Coerces JSON-stringified objects back to objects. */
const stringToObject = <T extends z.ZodTypeAny>(schema: T) =>
	z.preprocess((val) => {
		if (typeof val === "string") {
			try {
				const parsed = JSON.parse(val);
				if (typeof parsed === "object" && parsed !== null && !Array.isArray(parsed)) {
					return parsed;
				}
			} catch {
				// not valid JSON
			}
		}
		return val;
	}, schema);

/** Coerces "true"/"false" strings to booleans. */
const stringToBool = z.preprocess((val) => {
	if (typeof val === "string") {
		const lower = val.toLowerCase();
		if (lower === "true") return true;
		if (lower === "false") return false;
	}
	return val;
}, z.boolean());

// ---------------------------------------------------------------------------
// Per-tool schemas
// ---------------------------------------------------------------------------

/** read_file: reads one or more files with optional detail level, line ranges, and pagination */
export const ReadFileArgs = z.object({
	paths: stringToArray(z.string()),
	detail: z.string().optional(),
	start_line: z.union([z.string(), z.number()]).transform((v) => Number(v)).pipe(z.number().int().positive().optional()).optional(),
	end_line: z.union([z.string(), z.number()]).transform((v) => Number(v)).pipe(z.number().int().positive().optional()).optional(),
	max_tokens: z.union([z.string(), z.number()]).transform((v) => Number(v)).pipe(z.number().int().positive().optional()).optional(),
	page: z.string().optional(),
	section: z.string().optional(),
	ranges: z.string().optional(),
}).strict();

/** write_to_file / new_rule: creates or overwrites a file */
export const WriteToFileArgs = z.object({
	path: z.string(),
	content: z.string(),
}).strict();

/** edit_file: batch edit with files array */
export const EditFileArgs = z.object({
	files: stringToArray(
		z.object({
			path: z.string(),
			edits: stringToArray(
				z.object({
					edit_type: z.string().optional(),
					anchor: z.string(),
					end_anchor: z.string().optional(),
					text: z.string(),
				}),
			),
		}),
	),
}).strict();

/** search_files: regex search across files */
export const SearchFilesArgs = z.object({
	paths: stringToArray(z.string()),
	regex: z.string(),
	file_pattern: z.string().optional(),
	context_lines: z.union([z.string(), z.number()]).transform((v) => Number(v)).pipe(z.number().int().min(0).optional()).optional(),
}).strict();

/** list_files: list directory contents */
export const ListFilesArgs = z.object({
	paths: stringToArray(z.string()),
	recursive: z.union([z.boolean(), z.string()]).transform((v) => {
		if (typeof v === "string") return v.toLowerCase() === "true";
		return v;
	}).pipe(z.boolean()).optional(),
}).strict();

/** execute_command: run shell commands or scripts */
export const ExecuteCommandArgs = z.object({
	commands: z.union([
		stringToArray(z.string()),
		z.string(),
	]).optional(),
	script: z.string().optional(),
	language: z.string().optional(),
}).strict().refine(
	(data) => data.commands !== undefined || data.script !== undefined,
	{ message: "Either 'commands' or 'script' must be provided" },
);

/** get_function: extract function implementations */
export const GetFunctionArgs = z.object({
	paths: stringToArray(z.string()),
	function_names: stringToArray(z.string()),
}).strict();

/** get_file_skeleton: extract file structure outline */
export const GetFileSkeletonArgs = z.object({
	paths: stringToArray(z.string()),
}).strict();

/** find_symbol_references: locate symbol references */
export const FindSymbolReferencesArgs = z.object({
	paths: stringToArray(z.string()),
	symbols: stringToArray(z.string()),
	find_type: z.enum(["definition", "reference", "both"]).optional(),
}).strict();

/** replace_symbol: replace AST symbols */
export const ReplaceSymbolArgs = z.object({
	replacements: stringToArray(
		z.object({
			path: z.string(),
			symbol: z.string(),
			text: z.string(),
			type: z.string().optional(),
		}),
	),
}).strict();

/** rename_symbol: rename symbols across files */
export const RenameSymbolArgs = z.object({
	paths: stringToArray(z.string()),
	existing_symbol: z.string(),
	new_symbol: z.string(),
}).strict();

/** diagnostics_scan: run diagnostics on files */
export const DiagnosticsScanArgs = z.object({
	paths: stringToArray(z.string()),
}).strict();

/** attempt_completion: signal task completion */
export const AttemptCompletionArgs = z.object({
	result: z.string(),
	command: z.string().optional(),
	proof_of_execution: z.string().optional(),
}).strict();

/** ask_followup_question: ask the user a question */
export const AskFollowupQuestionArgs = z.object({
	question: z.string(),
	options: z.string().optional(),
}).strict();

/** browser_action: control a headless browser */
export const BrowserActionArgs = z.object({
	action: z.enum(["launch", "click", "type", "scroll_down", "scroll_up", "close"]),
	url: z.string().optional(),
	coordinate: z.string().optional(),
	text: z.string().optional(),
}).strict();

/** new_task: start a new sub-task */
export const NewTaskArgs = z.object({
	context: z.string(),
}).strict();

/** plan_mode_respond: respond in plan mode */
export const PlanModeRespondArgs = z.object({
	response: z.string(),
	options: z.string().optional(),
	needs_more_exploration: z.union([z.boolean(), z.string()]).transform((v) => {
		if (typeof v === "string") return v.toLowerCase() === "true";
		return v;
	}).pipe(z.boolean()).optional(),
}).strict();

/** web_fetch: fetch web content */
export const WebFetchArgs = z.object({
	url: z.string(),
	prompt: z.string(),
}).strict();

/** web_search: search the web */
export const WebSearchArgs = z.object({
	query: z.string(),
	allowed_domains: z.string().optional(),
	blocked_domains: z.string().optional(),
}).strict();

/** condense: condense conversation context */
export const CondenseArgs = z.object({
	context: z.string(),
}).strict();

/** summarize_task: summarize and compact context */
export const SummarizeTaskArgs = z.object({
	context: z.string(),
	required_files: stringToArray(z.string()).optional(),
}).strict();

/** report_bug: create a GitHub issue */
export const ReportBugArgs = z.object({
	title: z.string(),
	what_happened: z.string(),
	steps_to_reproduce: z.string(),
	api_request_output: z.string(),
	additional_context: z.string(),
}).strict();

/** use_skill: activate a skill */
export const UseSkillArgs = z.object({
	skill_name: z.string(),
}).strict();

/** list_skills: list available skills (no required args) */
export const ListSkillsArgs = z.object({}).strict();

/** use_subagents: delegate to subagent */
export const UseSubagentsArgs = z.object({
	context: z.string().optional(),
	prompt: z.string().optional(),
	prompt_1: z.string().optional(),
	prompt_2: z.string().optional(),
	prompt_3: z.string().optional(),
	prompt_4: z.string().optional(),
	prompt_5: z.string().optional(),
	include_history: z.union([z.boolean(), z.string()]).optional(),
	timeout: z.union([z.string(), z.number()]).transform((v) => {
		if (v === undefined || v === "") return undefined;
		const n = Number(v);
		return isNaN(n) ? undefined : n;
	}).optional(),
	max_turns: z.union([z.string(), z.number()]).transform((v) => {
		if (v === undefined || v === "") return undefined;
		const n = Number(v);
		return isNaN(n) ? undefined : n;
	}).optional(),
}).strict();

/** dirac_undo: undo last turn's file edits */
export const DiracUndoArgs = z.object({}).strict();

// ---------------------------------------------------------------------------
// Schema registry (maps DiracDefaultTool enum values → Zod schemas)
// ---------------------------------------------------------------------------
import { DiracDefaultTool } from "@/shared/tools";

/** Map from tool name to its Zod argument schema. */
export const TOOL_SCHEMAS: Partial<Record<DiracDefaultTool, z.ZodTypeAny>> = {
	[DiracDefaultTool.FILE_READ]: ReadFileArgs,
	[DiracDefaultTool.FILE_NEW]: WriteToFileArgs,
	[DiracDefaultTool.NEW_RULE]: WriteToFileArgs, // same shape
	[DiracDefaultTool.EDIT_FILE]: EditFileArgs,
	[DiracDefaultTool.SEARCH]: SearchFilesArgs,
	[DiracDefaultTool.LIST_FILES]: ListFilesArgs,
	[DiracDefaultTool.BASH]: ExecuteCommandArgs,
	[DiracDefaultTool.GET_FUNCTION]: GetFunctionArgs,
	[DiracDefaultTool.GET_FILE_SKELETON]: GetFileSkeletonArgs,
	[DiracDefaultTool.FIND_SYMBOL_REFERENCES]: FindSymbolReferencesArgs,
	[DiracDefaultTool.REPLACE_SYMBOL]: ReplaceSymbolArgs,
	[DiracDefaultTool.RENAME_SYMBOL]: RenameSymbolArgs,
	[DiracDefaultTool.DIAGNOSTICS_SCAN]: DiagnosticsScanArgs,
	[DiracDefaultTool.ATTEMPT]: AttemptCompletionArgs,
	[DiracDefaultTool.ASK]: AskFollowupQuestionArgs,
	[DiracDefaultTool.BROWSER]: BrowserActionArgs,
	[DiracDefaultTool.NEW_TASK]: NewTaskArgs,
	[DiracDefaultTool.PLAN_MODE]: PlanModeRespondArgs,
	[DiracDefaultTool.WEB_FETCH]: WebFetchArgs,
	[DiracDefaultTool.WEB_SEARCH]: WebSearchArgs,
	[DiracDefaultTool.CONDENSE]: CondenseArgs,
	[DiracDefaultTool.SUMMARIZE_TASK]: SummarizeTaskArgs,
	[DiracDefaultTool.REPORT_BUG]: ReportBugArgs,
	[DiracDefaultTool.USE_SKILL]: UseSkillArgs,
	[DiracDefaultTool.LIST_SKILLS]: ListSkillsArgs,
	[DiracDefaultTool.USE_SUBAGENTS]: UseSubagentsArgs,
	[DiracDefaultTool.DIRAC_UNDO]: DiracUndoArgs,
};
