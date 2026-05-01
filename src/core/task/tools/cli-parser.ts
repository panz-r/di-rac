import { parse as shellParse } from "shell-quote"
import type { DiracDefaultTool } from "@/shared/tools"

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface CliFlag {
	name: string // --start-line
	short?: string // -s
	param: string // block.params key (start_line)
	type: "string" | "boolean" | "integer"
	required?: boolean
	repeatable?: boolean
}

export interface CliPositional {
	name: string // display name for errors
	param: string // block.params key
	required: boolean
	multi?: boolean // consume remaining positionals into array
	join?: boolean // join multi-tokens into single string (instead of array)
}

export interface CliSchema {
	positionals?: CliPositional[]
	flags?: CliFlag[]
	transform?: (raw: Record<string, any>) => Record<string, any>
	/** Custom parser — bypasses default positional/flag parsing entirely. */
	parse?: (input: string) => Record<string, any>
	/** If true, chain operators (;, &&, ||) are NOT split and pass through raw. */
	noChainSplit?: boolean
}

// ---------------------------------------------------------------------------
// Universal flags — available on every CLI-migrated tool
// These use _-prefixed param keys so they never collide with tool-specific params.
// ---------------------------------------------------------------------------

export type ChainOperator = ";" | "&&" | "||"

export interface CommandSegment {
	command: string
	operator: ChainOperator | null // operator BEFORE this segment (null for first)
}

// ---------------------------------------------------------------------------
// Chain splitting — respects quoting
// ---------------------------------------------------------------------------

/**
 * Splits a command string by ;, &&, || and newlines into segments.
 * Respects single and double quoting so operators inside quoted strings
 * are treated as literal text.
 */
export function splitCommandChain(input: string): CommandSegment[] {
	const segments: CommandSegment[] = []
	let current = ""
	let i = 0

	while (i < input.length) {
		const ch = input[i]

		// Skip whitespace between segments
		if (ch === " " || ch === "\t") {
			current += ch
			i++
			continue
		}

		// Newline acts like ;
		if (ch === "\n") {
			const trimmed = current.trim()
			if (trimmed) segments.push({ command: trimmed, operator: null })
			current = ""
			i++
			continue
		}

		// Quoted string — consume until closing quote
		if (ch === "'" || ch === '"') {
			const quote = ch
			current += ch
			i++
			while (i < input.length && input[i] !== quote) {
				if (input[i] === "\\" && quote === '"' && i + 1 < input.length) {
					current += input[i] + input[i + 1]
					i += 2
				} else {
					current += input[i]
					i++
				}
			}
			if (i < input.length) {
				current += input[i] // closing quote
				i++
			}
			continue
		}

		// Check for operators: &&, ||, ;
		if (ch === "&" && i + 1 < input.length && input[i + 1] === "&") {
			const trimmed = current.trim()
			if (trimmed) segments.push({ command: trimmed, operator: null })
			current = ""
			i += 2
			peekSetOperator(segments, "&&")
			continue
		}
		if (ch === "|" && i + 1 < input.length && input[i + 1] === "|") {
			const trimmed = current.trim()
			if (trimmed) segments.push({ command: trimmed, operator: null })
			current = ""
			i += 2
			peekSetOperator(segments, "||")
			continue
		}
		if (ch === ";") {
			const trimmed = current.trim()
			if (trimmed) segments.push({ command: trimmed, operator: null })
			current = ""
			i++
			peekSetOperator(segments, ";")
			continue
		}

		current += ch
		i++
	}

	const trimmed = current.trim()
	if (trimmed) segments.push({ command: trimmed, operator: null })

	return segments
}

/** Set the operator on the last segment (will apply to the NEXT segment pushed). */
function peekSetOperator(segments: CommandSegment[], op: ChainOperator): void {
	if (segments.length > 0) {
		segments[segments.length - 1].operator = op
	}
}

// ---------------------------------------------------------------------------
// Flag helpers
// ---------------------------------------------------------------------------

function isFlag(token: string): boolean {
	return typeof token === "string" && token.startsWith("-") && token.length > 1 && !/^-\d/.test(token)
}

function isLongFlag(token: string): boolean {
	return typeof token === "string" && token.startsWith("--")
}

// ---------------------------------------------------------------------------
// Single-command parser
// ---------------------------------------------------------------------------

/**
 * Parse a single CLI command string (no chain operators) into structured
 * params matching the tool's CliSchema. Returns null if the tool has no
 * CLI schema (not yet migrated).
 */
export function parseCliCommand(toolName: string, input: string): Record<string, any> | null {
	const schema = CLI_SCHEMAS[toolName as DiracDefaultTool]
	if (!schema) return null

	// Empty input for zero-param tools
	if (!input.trim()) return {}

	// Custom parser override
	if (schema.parse) return schema.parse(input)

	// Build lookup maps — tool flags + universal flags
	const longFlags = new Map<string, CliFlag>()
	const shortFlags = new Map<string, CliFlag>()
	for (const flag of (schema.flags ?? [])) {
		longFlags.set(flag.name, flag)
		if (flag.short) shortFlags.set(flag.short, flag)
	}

	// Tokenize
	const tokens = shellParse(input) as string[]

	// Split into positionals and flags
	const positionalTokens: string[] = []
	let i = 0

	// Collect positionals (stop at first flag or end)
	while (i < tokens.length && !isFlag(tokens[i])) {
		positionalTokens.push(tokens[i])
		i++
	}

	// Parse flags
	const result: Record<string, any> = {}
	while (i < tokens.length) {
		const token = tokens[i]

		if (!isFlag(token)) {
			i++
			continue
		}

		let flag: CliFlag | undefined
		if (isLongFlag(token)) {
			flag = longFlags.get(token)
		} else {
			flag = shortFlags.get(token)
		}

		if (!flag) {
			i++
			if (i < tokens.length && !isFlag(tokens[i])) i++
			continue
		}

		if (flag.type === "boolean") {
			result[flag.param] = true
			i++
		} else {
			i++
			if (i < tokens.length) {
				let value: string | number = tokens[i]
				if (flag.type === "integer") {
					value = parseInt(value, 10)
					if (isNaN(value)) value = tokens[i]
				}
				if (flag.repeatable) {
					if (!result[flag.param]) result[flag.param] = []
					result[flag.param].push(value)
				} else {
					result[flag.param] = value
				}
				i++
			}
		}
	}

	// Assign positionals
	const posDefs = schema.positionals ?? []
	for (let p = 0; p < posDefs.length; p++) {
		const posDef = posDefs[p]
		if (posDef.multi) {
			const tokens = positionalTokens.slice(p)
			result[posDef.param] = posDef.join ? tokens.join(" ") : tokens
			break
		} else if (p < positionalTokens.length) {
			result[posDef.param] = positionalTokens[p]
		}
	}

	return schema.transform ? schema.transform(result) : result
}

/**
 * Returns true if the tool has a CLI schema registered (i.e., it's migrated).
 */
export function hasCliSchema(toolName: string): boolean {
	return toolName in CLI_SCHEMAS
}

export function getCliSchema(toolName: string): CliSchema | undefined {
	return CLI_SCHEMAS[toolName as DiracDefaultTool]
}

/**
 * Returns true if the tool's CLI schema opts out of chain splitting.
 * Used for tools like execute_command that handle shell operators internally.
 */
export function shouldChainSplit(toolName: string): boolean {
	const schema = CLI_SCHEMAS[toolName as DiracDefaultTool]
	return schema ? !schema.noChainSplit : true
}

// ---------------------------------------------------------------------------
// Custom parser for execute_command
// ---------------------------------------------------------------------------

function parseExecuteCommand(input: string): Record<string, any> {
	const result: Record<string, any> = {}

	// Extract --language flag first
	const langMatch = input.match(/--language\s+(['"]?)(.+?)\1(?:\s|$)/)
	if (langMatch) {
		result.language = langMatch[2].trim()
	}

	// Extract --script flag (quoted string or rest of line)
	const scriptMatch = input.match(/--script\s+(['"])([\s\S]*?)\1/)
	if (scriptMatch) {
		result.script = scriptMatch[2]
		return result
	}
	// Unquoted script (to end of line)
	const scriptMatch2 = input.match(/--script\s+(.+)$/s)
	if (scriptMatch2) {
		result.script = scriptMatch2[1].trim()
		return result
	}

	// Everything else (minus --language) is the command string
	let cmdStr = input
	if (langMatch) {
		cmdStr = cmdStr.replace(langMatch[0], " ").trim()
	}
	cmdStr = cmdStr.trim()
	if (cmdStr) {
		result.commands = cmdStr
	}

	return result
}

// ---------------------------------------------------------------------------
// Custom parser for subagent
// ---------------------------------------------------------------------------

function parseSubagentCommand(input: string): Record<string, any> {
	const result: Record<string, any> = {}
	const tokens = shellParse(input) as string[]
	const prompts: string[] = []
	let i = 0

	while (i < tokens.length) {
		if (tokens[i] === "--prompt" && i + 1 < tokens.length) {
			prompts.push(tokens[i + 1])
			i += 2
		} else if (tokens[i] === "--include-history") {
			result.include_history = true
			i++
		} else if (tokens[i] === "--timeout" && i + 1 < tokens.length) {
			result.timeout = parseInt(tokens[i + 1], 10)
			i += 2
		} else if (tokens[i] === "--max-turns" && i + 1 < tokens.length) {
			result.max_turns = parseInt(tokens[i + 1], 10)
			i += 2
		} else {
			i++
		}
	}

	for (let p = 0; p < Math.min(prompts.length, 5); p++) {
		result[`prompt_${p + 1}`] = prompts[p]
	}

	return result
}

// ---------------------------------------------------------------------------
// Schema registry
// ---------------------------------------------------------------------------

const CLI_SCHEMAS: Partial<Record<DiracDefaultTool, CliSchema>> = {
	// ── Batch 1 ──────────────────────────────────────────────────────────────

	read_file: {
		positionals: [{ name: "path", param: "paths", required: true, multi: true }],
		flags: [
			{ name: "--detail", param: "detail", type: "string" },
			{ name: "--start-line", param: "start_line", type: "integer" },
			{ name: "--end-line", param: "end_line", type: "integer" },
			{ name: "--max-tokens", param: "max_tokens", type: "integer" },
			{ name: "--page", param: "page", type: "string" },
			{ name: "--section", param: "section", type: "string" },
			{ name: "--ranges", param: "ranges", type: "string" },
		],
	},

	search_files: {
		positionals: [{ name: "path", param: "paths", required: true, multi: true }],
		flags: [
			{ name: "--regex", param: "regex", type: "string", required: true },
			{ name: "--file-pattern", param: "file_pattern", type: "string" },
			{ name: "--context-lines", param: "context_lines", type: "integer" },
		],
	},

	list_files: {
		positionals: [{ name: "path", param: "paths", required: true, multi: true }],
		flags: [{ name: "--recursive", param: "recursive", type: "boolean" }],
	},

	search_symbols: {
		positionals: [{ name: "query", param: "query", required: true }],
		flags: [
			{ name: "--kind", param: "kind", type: "string" },
			{ name: "--max-results", param: "max_results", type: "integer" },
		],
	},

	repo_map: {},
	list_skills: {},

	// ── Batch 2 ──────────────────────────────────────────────────────────────

	// execute_command <command> [--script TEXT] [--language LANG]
	// Shell operators (&&, ||, ;) are passed through — no chain splitting.
	execute_command: {
		noChainSplit: true,
		parse: parseExecuteCommand,
	},

	// get_function <path>... --fn <name>...
	get_function: {
		positionals: [{ name: "path", param: "paths", required: true, multi: true }],
		flags: [
			{ name: "--fn", param: "function_names", type: "string", required: true, repeatable: true },
		],
	},

	// get_file_skeleton <path>...
	get_file_skeleton: {
		positionals: [{ name: "path", param: "paths", required: true, multi: true }],
	},

	// diagnostics_scan <path>...
	diagnostics_scan: {
		positionals: [{ name: "path", param: "paths", required: true, multi: true }],
	},

	// expand_symbol <path> --symbol <handle>
	expand_symbol: {
		positionals: [{ name: "path", param: "path", required: true }],
		flags: [
			{ name: "--symbol", param: "symbol", type: "string", required: true },
		],
	},

	// find_symbol_references <path>... --symbol <name>... [--find-type TYPE]
	find_symbol_references: {
		positionals: [{ name: "path", param: "paths", required: true, multi: true }],
		flags: [
			{ name: "--symbol", param: "symbols", type: "string", required: true, repeatable: true },
			{ name: "--find-type", param: "find_type", type: "string" },
		],
	},

	// rename_symbol <path>... --old <name> --new <name>
	rename_symbol: {
		positionals: [{ name: "path", param: "paths", required: true, multi: true }],
		flags: [
			{ name: "--old", param: "existing_symbol", type: "string", required: true },
			{ name: "--new", param: "new_symbol", type: "string", required: true },
		],
	},

	// replace_symbol <path> --symbol <name> --text <content> [--type KIND]
	// Single replacement per call; use chain operators for multiple.
	replace_symbol: {
		positionals: [{ name: "path", param: "path", required: true }],
		flags: [
			{ name: "--symbol", param: "symbol", type: "string", required: true },
			{ name: "--text", param: "text", type: "string", required: true },
			{ name: "--type", param: "type", type: "string" },
		],
		transform: (raw) => ({
			replacements: [{
				path: raw.path,
				symbol: raw.symbol,
				text: raw.text,
				...(raw.type ? { type: raw.type } : {}),
			}],
		}),
	},

	// ── Batch 3 ──────────────────────────────────────────────────────────────

	// edit_file <path> --anchor <id> --content <text> [--end-anchor <id>] [--edit-type TYPE]
	// Single edit per call; use chain operators for batch edits.
	edit_file: {
		positionals: [{ name: "path", param: "path", required: true }],
		flags: [
			{ name: "--anchor", param: "anchor", type: "string", required: true },
			{ name: "--content", param: "text", type: "string", required: true },
			{ name: "--end-anchor", param: "end_anchor", type: "string" },
			{ name: "--edit-type", param: "edit_type", type: "string" },
		],
		transform: (raw) => ({
			files: [{
				path: raw.path,
				edits: [{
					anchor: raw.anchor,
					text: raw.text,
					...(raw.end_anchor ? { end_anchor: raw.end_anchor } : {}),
					...(raw.edit_type ? { edit_type: raw.edit_type } : {}),
				}],
			}],
		}),
	},

	// write_to_file <path> --content <text>
	write_to_file: {
		positionals: [{ name: "path", param: "path", required: true }],
		flags: [
			{ name: "--content", param: "content", type: "string", required: true },
		],
	},

	// browser_action <action> [--url URL] [--coordinate X,Y] [--text TEXT]
	browser_action: {
		positionals: [{ name: "action", param: "action", required: true }],
		flags: [
			{ name: "--url", param: "url", type: "string" },
			{ name: "--coordinate", param: "coordinate", type: "string" },
			{ name: "--text", param: "text", type: "string" },
		],
	},

	// use_subagents --prompt TEXT --prompt TEXT [--include-history] [--timeout SEC] [--max-turns N]
	use_subagents: {
		parse: parseSubagentCommand,
	},

	// ask_followup_question <question> [--options JSON]
	ask_followup_question: {
		positionals: [{ name: "question", param: "question", required: true }],
		flags: [
			{ name: "--options", param: "options", type: "string" },
		],
	},

	// attempt_completion <result> [--command CMD]
	attempt_completion: {
		positionals: [{ name: "result", param: "result", required: true }],
		flags: [
			{ name: "--command", param: "command", type: "string" },
		],
	},

	// web_fetch <url> --prompt <text>
	web_fetch: {
		positionals: [{ name: "url", param: "url", required: true }],
		flags: [
			{ name: "--prompt", param: "prompt", type: "string", required: true },
		],
	},

	// web_search <query> [--allowed-domains JSON] [--blocked-domains JSON]
	web_search: {
		positionals: [{ name: "query", param: "query", required: true }],
		flags: [
			{ name: "--allowed-domains", param: "allowed_domains", type: "string" },
			{ name: "--blocked-domains", param: "blocked_domains", type: "string" },
		],
	},

	// plan_mode_respond <response> [--needs-more-exploration]
	plan_mode_respond: {
		positionals: [{ name: "response", param: "response", required: true }],
		flags: [
			{ name: "--needs-more-exploration", param: "needs_more_exploration", type: "boolean" },
		],
	},

	// new_task <context>
	new_task: {
		positionals: [{ name: "context", param: "context", required: true }],
	},

	// summarize_task <context> [--required-files PATH...]
	summarize_task: {
		positionals: [{ name: "context", param: "context", required: true }],
		flags: [
			{ name: "--required-files", param: "required_files", type: "string", repeatable: true },
		],
	},

	// compact <context> [--required-files PATH...]
	compact: {
		positionals: [{ name: "context", param: "context", required: true }],
		flags: [
			{ name: "--required-files", param: "required_files", type: "string", repeatable: true },
		],
	},

	// generate_explanation <title> --from-ref <ref> [--to-ref <ref>]
	generate_explanation: {
		positionals: [{ name: "title", param: "title", required: true }],
		flags: [
			{ name: "--from-ref", param: "from_ref", type: "string", required: true },
			{ name: "--to-ref", param: "to_ref", type: "string" },
		],
	},

	// use_skill <skill_name>
	use_skill: {
		positionals: [{ name: "skill_name", param: "skill_name", required: true }],
	},

	// tool_search [query] [--capabilities] [--llms-brief]
	tool_search: {
		positionals: [{ name: "query", param: "query", required: false }],
	},

	// dirac_outputs [file] [--clear]
	dirac_outputs: {
		positionals: [{ name: "file", param: "file", required: false }],
		flags: [{ name: "--clear", param: "clear", type: "boolean" }],
	},
}
