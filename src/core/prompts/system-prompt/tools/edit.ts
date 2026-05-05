import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.EDIT_FILE

export const edit: DiracToolSpec = {
	id,
	name: "edit",
	description: `Edit files using hash-anchored line references (e.g. "a3|def foo():"). Types: replace (anchor to end-anchor), insert_after, insert_before.

Examples:
  edit src/auth.ts --anchor "a3|def login():" --end-anchor "k7|  pass" --content "def login():\\n  ..."
  edit src/auth.ts --anchor "b2|class Auth:" --content "  def foo():\\n    pass" --type insert_after

Chain: edit a.ts --anchor 'a3|...' --content '...'; edit b.ts --anchor 'b2|...' --content '...'
Anchor format: hash|content (e.g. "a3|def foo():"). Use read --detail outline or symbols search to get anchors before editing. Response: OK | edits:N | tokens:N. Diffs follow. Verify with read after editing.
Universal flags: --dry-run (edit temp file, show diff), --retry N.
Typical: edit src/file.ts --anchor 'a3|def foo' --content 'new body'`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
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
