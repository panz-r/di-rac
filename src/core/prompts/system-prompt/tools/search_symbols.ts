import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.SEARCH_SYMBOLS

export const search_symbols: DiracToolSpec = {
	id,
	name: "search_symbols",
	description:
		"Searches for symbols (functions, classes, interfaces) by name pattern across the entire project using structural indexing. Returns handles and signatures. This is more precise than grep for finding definitions.",
	parameters: [
		{
			name: "query",
			required: true,
			type: "string",
			instruction: "The name pattern to search for (case-insensitive).",
			usage: '"AuthService"',
		},
		{
			name: "kind",
			required: false,
			type: "string",
			instruction: "Optional filter for symbol kind: 'function', 'class', 'interface', 'method', etc.",
			usage: '"function"',
		},
		{
			name: "max_results",
			required: false,
			type: "integer",
			instruction: "Maximum number of results to return (default: 20).",
			usage: "10",
		},
	],
}
