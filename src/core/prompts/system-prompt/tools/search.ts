import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.SEARCH

export const search: DiracToolSpec = {
	id,
	name: "search",
	description: `Regex search across files. Skips .git, node_modules, build/, binaries. Returns first 30 matches with file, line, and context. Use symbols search for code navigation; use this for text patterns, config values, comments.

Examples:
  search src/ --pattern "TODO|FIXME" --context 2
  search config/ --pattern "API_KEY" --glob "*.env"

Response: OK | matches:N | files:N | hint:refinements | tokens:N
	Matches follow: file:line:context (one per line, max 30).
Note: --context 0-5. Results auto-truncated at 30 matches; narrow your pattern or path if partial.
Typical: search src/ --pattern 'TODO|FIXME'`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for search. Use --pattern and optional filter flags.",
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
