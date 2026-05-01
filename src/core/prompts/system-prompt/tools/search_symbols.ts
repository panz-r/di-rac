import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.SEARCH_SYMBOLS

export const search_symbols: DiracToolSpec = {
	id,
	name: "search_symbols",
	description: `Search for symbols (functions, classes, interfaces) by name pattern across the project using structural indexing. Returns handles and signatures. More precise than grep for finding definitions.

Usage: search_symbols <query> [options]

Positional:
  query               Name pattern to search for (case-insensitive)

Options:
  --kind TYPE         Filter by symbol kind: function, class, interface, method, etc.
  --max-results N     Maximum results to return (default 20).

Examples:
  search_symbols AuthService
  search_symbols login --kind function
  search_symbols "handle.*" --max-results 10`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for search_symbols.",
			usage: '"AuthService" --kind function',
		},
	],
	metadata: {
		tags: ["code", "search", "symbol"],
		category: "code-intel",
		concurrency: "parallel-safe",
		safety: ["read"],
		outputSize: "small",
		llmsBrief: "Search workspace symbols by query",
		compactionSafety: "summarizable",
	},
}
