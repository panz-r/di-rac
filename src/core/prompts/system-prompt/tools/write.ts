import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.FILE_NEW

export const write: DiracToolSpec = {
	id,
	name: "write",
	description: `Create or overwrite a file. Auto-creates parent directories.

Example: write src/new.ts --content "export function foo() { ... }"

Chain: write a.ts --content '...'; write b.ts --content '...'
Response: OK | lines:N | path:<path> | tokens:N
Fails when: path is a directory, content missing, disk full.
If fails: verify path is a file (not dir), ensure --content is provided, check disk space.
After results: read the file to verify content, or use edit for targeted refinements.
Good: file created/overwritten with expected content. Bad: directory path (specify a file), missing content (ensure --content is set).
Don't use for: editing existing files (use edit), reading content (use read).
Output example: OK | lines:5 | path:src/new.ts | tokens:20
Universal flags: --dry-run (write to temp file, show diff), --retry N.
Typical: write src/new.ts --content 'export const X = ...'`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for write. Includes path and --content.",
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
