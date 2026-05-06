import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.LIST_FILES

export const repo: DiracToolSpec = {
	id,
	name: "repo",
	description: `Get repository structural overview. --detail: summary (default, top symbols per file), files (all files with line counts), skeleton (all defs). Optional paths filter directories. For file content, use read. For searching text patterns, use search.

Example: repo --detail files src/

Response: OK | files:N | lines:N | symbols:N | detail:<summary|files|skeleton> | tokens:N
	Content follows. Structure varies by detail level.
Typical: repo --detail files src/`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for repo. Use --detail to control output depth.",
			usage: "repo --detail files src/",
		},
	],
	metadata: {
		tags: ["repo", "structure", "overview"],
		category: "navigation",
		concurrency: "parallel-safe",
		safety: ["read"],
		outputSize: "medium",
		llmsBrief: "Repository structural overview",
		compactionSafety: "discardable",
	},
}
