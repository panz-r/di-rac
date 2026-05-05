import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.TOOL_SEARCH

export const tools: DiracToolSpec = {
	id,
	name: "tools",
	description: `Discover available tools. Optional keyword to filter.

Examples:
  tools
  tools file
  tools edit

Response: OK | tools:N | <list> | tokens:N`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for tools. Lists available tools.",
			usage: "file",
		},
	],
	metadata: {
		tags: ["meta", "discovery"],
		category: "meta",
		concurrency: "parallel-safe",
		safety: ["read"],
		outputSize: "small",
		llmsBrief: "Discover available tools",
		compactionSafety: "discardable",
	},
}
