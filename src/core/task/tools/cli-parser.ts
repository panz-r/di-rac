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
}

export interface CliSchema {
	positionals?: CliPositional[]
	flags?: CliFlag[]
	transform?: (raw: Record<string, any>) => Record<string, any>
}

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
			// Mark next segment's operator
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
	// The operator goes between segments. We track it by setting it on
	// the most recently pushed segment. During execution, the operator
	// on segment[i] means "the condition for executing segment[i]".
	// The first segment always has operator null (unconditional).
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

	// Build lookup maps
	const longFlags = new Map<string, CliFlag>()
	const shortFlags = new Map<string, CliFlag>()
	for (const flag of schema.flags ?? []) {
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
			result[posDef.param] = positionalTokens.slice(p)
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

// ---------------------------------------------------------------------------
// Schema registry
// ---------------------------------------------------------------------------

const CLI_SCHEMAS: Partial<Record<DiracDefaultTool, CliSchema>> = {
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
}
