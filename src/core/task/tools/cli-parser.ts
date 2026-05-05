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

	// Custom parser override — but still extract universal flags first
	if (schema.parse) {
		const result = schema.parse(input)
		// Extract universal flags from raw input
		if (/\b--clarify\b/.test(input)) result.clarify = true
		if (/\b--no-auto-correct\b/.test(input)) result.autoCorrect = false
		if (/\b--dry-run\b/.test(input)) result.dryRun = true
		if (/\b--verify\b/.test(input)) result.verify = true
		const retryMatch = input.match(/\b--retry\s+(\d+)/)
		if (retryMatch) result.retry = parseInt(retryMatch[1], 10)
		return result
	}

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

			// Universal flags: --clarify, --retry
			if (token === "--clarify") {
				result.clarify = true
				i++
				continue
			}
			if (token === "--retry" && i + 1 < tokens.length) {
				const n = parseInt(tokens[i + 1], 10)
				if (!isNaN(n) && n >= 1) result.retry = Math.min(n, 5)
				i += 2
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
// Custom parser for symbols subcommands
// ---------------------------------------------------------------------------

function parseSymbolsCommand(input: string): Record<string, any> {
	const tokens = shellParse(input) as string[]
	if (tokens.length === 0) return { subcommand: "search" }

	const subcommand = tokens[0]
	const rest = input.slice(input.indexOf(tokens[0]) + tokens[0].length).trim()

	switch (subcommand) {
		case "search": {
			const result: Record<string, any> = { subcommand: "search" }
			let i = 1
			const paths: string[] = []
			while (i < tokens.length) {
				if (tokens[i] === "--name" && i + 1 < tokens.length) { result.query = tokens[++i]; i++ }
				else if (tokens[i] === "--kind" && i + 1 < tokens.length) { result.kind = tokens[++i]; i++ }
				else if (tokens[i] === "--max-results" && i + 1 < tokens.length) { result.max_results = parseInt(tokens[++i], 10); i++ }
				else if (!tokens[i].startsWith("-")) { paths.push(tokens[i]); i++ }
				else { i++ }
			}
			if (paths.length > 0) result.paths = paths
			return result
		}
		case "replace": {
			const result: Record<string, any> = { subcommand: "replace" }
			let i = 1
			const paths: string[] = []
			while (i < tokens.length) {
				if (tokens[i] === "--name" && i + 1 < tokens.length) { result.symbol = tokens[++i]; i++ }
				else if (tokens[i] === "--text" && i + 1 < tokens.length) { result.text = tokens[++i]; i++ }
				else if (tokens[i] === "--type" && i + 1 < tokens.length) { result.type = tokens[++i]; i++ }
				else if (!tokens[i].startsWith("-")) { paths.push(tokens[i]); i++ }
				else { i++ }
			}
			if (paths.length > 0) result.path = paths[0]
			if (result.symbol && result.text) {
				result.replacements = [{ path: result.path, symbol: result.symbol, text: result.text }]
			}
			return result
		}
		case "rename": {
			const result: Record<string, any> = { subcommand: "rename" }
			let i = 1
			const paths: string[] = []
			while (i < tokens.length) {
				if (tokens[i] === "--old" && i + 1 < tokens.length) { result.existing_symbol = tokens[++i]; i++ }
				else if (tokens[i] === "--new" && i + 1 < tokens.length) { result.new_symbol = tokens[++i]; i++ }
				else if (!tokens[i].startsWith("-")) { paths.push(tokens[i]); i++ }
				else { i++ }
			}
			if (paths.length > 0) result.paths = paths
			return result
		}
		case "refs": {
			const result: Record<string, any> = { subcommand: "refs" }
			let i = 1
			const paths: string[] = []
			const names: string[] = []
			while (i < tokens.length) {
				if (tokens[i] === "--name" && i + 1 < tokens.length) { names.push(tokens[++i]); i++ }
				else if (tokens[i] === "--find-type" && i + 1 < tokens.length) { result.find_type = tokens[++i]; i++ }
				else if (!tokens[i].startsWith("-")) { paths.push(tokens[i]); i++ }
				else { i++ }
			}
			if (paths.length > 0) result.paths = paths
			if (names.length > 0) result.symbols = names
			return result
		}
		default:
			return { subcommand: "search", query: rest }
	}
}

// ---------------------------------------------------------------------------
// Schema registry
// ---------------------------------------------------------------------------

const CLI_SCHEMAS: Partial<Record<string, CliSchema>> = {
	// read <path>... [options]
	read: {
		positionals: [{ name: "path", param: "paths", required: true, multi: true }],
		flags: [
			{ name: "--detail", param: "detail", type: "string" },
			{ name: "--start-line", param: "start_line", type: "integer" },
			{ name: "--end-line", param: "end_line", type: "integer" },
			{ name: "--max-tokens", param: "max_tokens", type: "integer" },
			{ name: "--page", param: "page", type: "string" },
			{ name: "--section", param: "section", type: "string" },
			{ name: "--ranges", param: "ranges", type: "string" },
			{ name: "--range", param: "ranges", type: "string" },
		],
	},

	// search <path>... --pattern PATTERN [options]
	search: {
		positionals: [{ name: "path", param: "paths", required: true, multi: true }],
		flags: [
			{ name: "--pattern", param: "regex", type: "string", required: true },
			{ name: "--regex", param: "regex", type: "string" },
			{ name: "--glob", param: "file_pattern", type: "string" },
			{ name: "--file-pattern", param: "file_pattern", type: "string" },
			{ name: "--context", param: "context_lines", type: "integer" },
			{ name: "--context-lines", param: "context_lines", type: "integer" },
		],
	},

	// repo [--detail LEVEL] [paths...]
	repo: {
		positionals: [{ name: "paths", param: "paths", required: false, multi: true }],
		flags: [
			{ name: "--detail", param: "detail", type: "string" },
			{ name: "--recursive", param: "recursive", type: "boolean" },
		],
	},

	list_skills: {},

	// bash <command>
	bash: {
		noChainSplit: true,
		parse: (input: string) => ({ command: input }),
	},

	// symbols <subcommand> [args...]
	symbols: {
		noChainSplit: true,
		parse: parseSymbolsCommand,
	},

	// edit <path> --anchor <id> --content <text> [options]
	edit: {
		positionals: [{ name: "path", param: "path", required: true }],
		flags: [
			{ name: "--anchor", param: "anchor", type: "string", required: true },
			{ name: "--content", param: "text", type: "string", required: true },
			{ name: "--end-anchor", param: "end_anchor", type: "string" },
			{ name: "--edit-type", param: "edit_type", type: "string" },
			{ name: "--type", param: "edit_type", type: "string" },
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

	// write <path> --content <text>
	write: {
		positionals: [{ name: "path", param: "path", required: true }],
		flags: [
			{ name: "--content", param: "content", type: "string", required: true },
		],
	},

	// browser_action <action> [options]
	browser_action: {
		positionals: [{ name: "action", param: "action", required: true }],
		flags: [
			{ name: "--url", param: "url", type: "string" },
			{ name: "--coordinate", param: "coordinate", type: "string" },
			{ name: "--text", param: "text", type: "string" },
		],
	},

	// use_subagents [options]
	use_subagents: {
		parse: parseSubagentCommand,
	},

	// ask <question> [--options A,B,C]
	ask: {
		positionals: [{ name: "question", param: "question", required: true }],
		flags: [
			{ name: "--options", param: "options", type: "string" },
		],
	},

	// done <result> [--cmd COMMAND]
	done: {
		positionals: [{ name: "result", param: "result", required: true }],
		flags: [
			{ name: "--cmd", param: "command", type: "string" },
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

	// web_search <query> [options]
	web_search: {
		positionals: [{ name: "query", param: "query", required: true }],
		flags: [
			{ name: "--allowed-domains", param: "allowed_domains", type: "string" },
			{ name: "--blocked-domains", param: "blocked_domains", type: "string" },
		],
	},

	// plan <response> [--explore]
	plan: {
		positionals: [{ name: "response", param: "response", required: true }],
		flags: [
			{ name: "--explore", param: "needs_more_exploration", type: "boolean" },
			{ name: "--needs-more-exploration", param: "needs_more_exploration", type: "boolean" },
		],
	},

	// task <context>
	task: {
		positionals: [{ name: "context", param: "context", required: true }],
	},

	// compact <context> [--keep PATH...]
	compact: {
		positionals: [{ name: "context", param: "context", required: true }],
		flags: [
			{ name: "--keep", param: "required_files", type: "string", repeatable: true },
			{ name: "--required-files", param: "required_files", type: "string", repeatable: true },
		],
	},

	// use_skill <skill_name>
	use_skill: {
		positionals: [{ name: "skill_name", param: "skill_name", required: true }],
	},

	// tools [query]
	tools: {
		positionals: [{ name: "query", param: "query", required: false }],
	},

	// memory [file] [--clear]
	memory: {
		positionals: [{ name: "file", param: "file", required: false }],
		flags: [{ name: "--clear", param: "clear", type: "boolean" }],
	},

	// recall <query>
	recall: {
		positionals: [{ name: "query", param: "query", required: true }],
	},
}
