import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.FILE_NEW

export const write_to_file: DiracToolSpec = {
	id,
	name: "write_to_file",
	description: `Creates a new file or completely overwrites an existing file. Automatically creates required directories.

Usage: write_to_file <path> --content <text>

Positional:
  path                File path to write

Options:
  --content TEXT      (required) Complete intended content of the file.

Examples:
  write_to_file src/new-module.ts --content "export function hello() { ... }"
  write_to_file config.json --content '{"key": "value"}'`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for write_to_file.",
			usage: 'src/new-module.ts --content "export function hello() { ... }"',
		},
	],
	metadata: {
		tags: ["file", "write", "create"],
		category: "file-io",
		concurrency: "sequential",
		safety: ["write"],
		supportsDryRun: true,
		supportsForce: true,
		outputSize: "small",
		llmsBrief: "Write or create files with full content replacement",
		compactionSafety: "essential",
	},
}
