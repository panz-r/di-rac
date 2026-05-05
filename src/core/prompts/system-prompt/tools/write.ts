import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.FILE_NEW

export const write: DiracToolSpec = {
	id,
	name: "write",
	description: `Create or overwrite a file. Auto-creates parent directories.

Examples:
  write src/new.ts --content "export function foo() { ... }"
  write config.json --content '{"key": "value"}'

Chain: write a.ts --content '...'; write b.ts --content '...'
Response: OK | lines:N | path:<path> | tokens:N
Universal flags: --dry-run (write to temp file, show diff), --retry N.
Typical: write src/new.ts --content 'export const X = ...'`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			usage: "src/new.ts --content 'export function foo() {}'",
		},
	],
	metadata: {
		tags: ["file", "write", "create"],
		category: "file",
		concurrency: "sequential",
		safety: ["write"],
		outputSize: "small",
		llmsBrief: "Create or overwrite files",
		compactionSafety: "discardable",
	},
}
