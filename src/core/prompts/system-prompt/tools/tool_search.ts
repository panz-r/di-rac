import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.TOOL_SEARCH

export const tool_search: DiracToolSpec = {
	id,
	name: "tool_search",
	description: `Discover and query available tools.

Usage: tool_search [query]

Positional:
  query            Optional keyword to search tool names, tags, and categories

Examples:
  tool_search
  tool_search authentication
  tool_search file`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for tool_search.",
			usage: "authentication",
		},
	],
	metadata: {
		tags: ["meta", "discovery"],
		category: "meta",
		concurrency: "parallel-safe",
		safety: ["read"],
		outputSize: "small",
		llmsBrief: "Discover and query available tools by keyword",
		compactionSafety: "discardable",
	},
}
