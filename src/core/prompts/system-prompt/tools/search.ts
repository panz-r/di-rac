import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.SEARCH

export const search: DiracToolSpec = {
	id,
	name: "search",
	description: `Search files with regex patterns. Skips .git, node_modules, build/, binaries. Returns first 30 matches with file, line, and context. Use for text patterns, config values, comments — not code navigation. For code navigation (functions, classes, imports), use symbols search --name instead.

Example: search --pattern "TODO|FIXME" --context 2

Response: OK | matches:N | files:N | hint:refinements | tokens:N
	Matches follow: file:line:context (one per line, max 30).
Note: path is optional (defaults to cwd). --context 0-5. Results auto-truncated at 30 matches; narrow your pattern or path if partial.
Typical: search --pattern 'TODO|FIXME'`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for search. Path is optional (defaults to cwd). Use --pattern and optional filter flags.",
			usage: "--pattern 'TODO|FIXME' --context 2",
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
