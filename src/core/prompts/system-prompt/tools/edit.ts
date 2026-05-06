import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.EDIT_FILE

export const edit: DiracToolSpec = {
	id,
	name: "edit",
	description: `Edit files using hash-anchored line references. Don't rewrite entire files — use targeted anchors. Don't edit without first reading anchors (e.g. "a3|def foo():"). Types: replace (anchor to end-anchor), insert_after, insert_before.

Example: edit src/auth.ts --anchor "a3|def login():" --end-anchor "k7|  pass" --content "def login():\\n  ..."

Chain: edit a.ts --anchor 'a3|...' --content '...'; edit b.ts --anchor 'b2|...' --content '...'
Anchor format: hash|content (e.g. "a3|def foo():"). Use read --detail outline or symbols search to get anchors before editing. Response: OK | edits:N | tokens:N. Diffs follow. Verify with read after editing.
Fails when: anchor not found (file changed since last read), end-anchor before start-anchor.
If fails: re-read the file to get current anchors, then retry. Use --dry-run to preview changes.
Don't use for: creating new files (use write), reading content (use read).
Universal flags: --dry-run (edit temp file, show diff), --retry N.
Typical: edit src/file.ts --anchor 'a3|def foo' --content 'new body'`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for edit. Use --anchor and --content flags. Chain edits with ;.",
			usage: "src/auth.ts --anchor 'a3|def login():' --content '...'",
		},
	],
	metadata: {
		tags: ["file", "edit"],
		category: "file",
		concurrency: "sequential",
		safety: ["write"],
		outputSize: "small",
		llmsBrief: "Edit files by anchor",
		compactionSafety: "discardable",
	},
}
