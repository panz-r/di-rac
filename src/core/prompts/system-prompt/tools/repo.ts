import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.LIST_FILES

export const repo: DiracToolSpec = {
	id,
	name: "repo",
	description: `Get repository structural overview. --detail: summary (default, top symbols per file), files (all files with line counts), skeleton (all defs). Optional paths filter directories.

Example: repo --detail files src/

Response: OK | files:N | lines:N | symbols:N | detail:<summary|files|skeleton> | tokens:N
	Content follows. Structure varies by detail level.
Fails when: path doesn't exist (returns empty), very large repos (--detail skeleton may be slow).
If fails: verify path with repo --detail files, or narrow to a subdirectory.
After results: read --detail outline on specific files to explore, or search for patterns within.
Good: files listed with expected structure. Bad: empty (wrong path), too many files (narrow with path filter).
Don't use for: file content (use read), text search (use search), specific definitions (use symbols).
Output example (files): OK | files:12 | detail:files | tokens:30
  src/auth.ts 142
  src/config.ts 58
  src/utils/helpers.ts 89
Tip: use path filter to limit scope (e.g. repo --detail files src/auth/). Use --detail summary instead of skeleton for large repos.
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
