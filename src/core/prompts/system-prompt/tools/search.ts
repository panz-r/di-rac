import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.SEARCH

export const search: DiracToolSpec = {
	id,
	name: "search",
	description: `Regex search across files. Skips .git, node_modules, build/. Use symbols search for code navigation; use this for text patterns, config values, comments.

Examples:
  search src/ --pattern "TODO|FIXME" --context 2
  search config/ --pattern "API_KEY" --glob "*.env"

Returns: first 30 matches. --context 0-5 for surrounding lines.
Typical: search src/ --pattern 'TODO|FIXME'`,
		contextRequirements: (ctx) => (ctx.toolCallCount ?? 99) >= 5,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			usage: "src/ --pattern 'TODO|FIXME' --context 2",
		},
	],
	metadata: {
		tags: ["search", "regex", "grep"],
		category: "search",
		concurrency: "parallel-safe",
		safety: ["read"],
		outputSize: "medium",
		llmsBrief: "Regex text search across files",
		compactionSafety: "discardable",
	},
}
