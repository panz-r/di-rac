import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.LIST_FILES

export const list_files: DiracToolSpec = {
	id,
	name: "list_files",
	description: `List files and directories within the specified paths. Skips non-useful content (.git, node_modules, build artifacts, etc.). Files sorted by most recently modified. Output includes line count per file. Do not use this to confirm files you just created.

Usage: list_files <path>... [options]

Positional:
  path            One or more directory or file paths (relative to CWD){{MULTI_ROOT_HINT}}

Options:
  --recursive     List files recursively.

Examples:
  list_files src/components src/utils
  list_files src/ --recursive`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for list_files.",
			usage: 'src/components --recursive',
		},
	],
	metadata: {
		tags: ["file", "directory", "list"],
		category: "file-io",
		concurrency: "parallel-safe",
		safety: ["read"],
		outputSize: "small",
		llmsBrief: "List files and directories with optional recursion",
		compactionSafety: "summarizable",
	},
}
