import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.FIND_SYMBOL_REFERENCES

export const find_symbol_references: DiracToolSpec = {
	id,
	name: "find_symbol_references",
	description: `Finds all exact AST references and invocations of one or more functions, classes, or variables across specified files or directories. Returns precise file paths.

Usage: find_symbol_references <path>... --symbol <name>... [options]

Positional:
  path                    Files or directories to search in

Options:
  --symbol NAME           (required, repeatable) Exact symbol names to find references for.
  --find-type TYPE        Filter: definition, reference, or both (default both).

Examples:
  find_symbol_references src/ --symbol login --symbol AuthService
  find_symbol_references src/ tests/ --symbol calculateTotal --find-type reference`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for find_symbol_references.",
			usage: 'src/ --symbol calculateTotal --symbol UserAccount',
		},
	],
	metadata: {
		tags: ["code", "references", "symbol"],
		category: "code-intel",
		concurrency: "parallel-safe",
		safety: ["read"],
		outputSize: "medium",
		llmsBrief: "Find all references to symbols across files",
		compactionSafety: "summarizable",
	},
}
