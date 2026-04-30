import { DiracDefaultTool } from "@/shared/tools"
import { getDelimiter } from "../../../../utils/line-hashing"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.EDIT_FILE

export const edit_file: DiracToolSpec = {
	id,
	name: "edit_file",
	description: `Edit a file by replacing, inserting after, or inserting before specific anchor lines.

EDIT TYPES:
1. replace (default): Replaces an inclusive range of lines from anchor to end_anchor.
2. insert_after: Inserts text immediately after the line specified by anchor. end_anchor is not used.
3. insert_before: Inserts text immediately before the line specified by anchor. end_anchor is not used.

ANCHOR RULES:
1. Anchors are short hash codes (e.g., "a3", "k7_1") followed by ${getDelimiter()} then the line content.
2. For 'replace', anchors are inclusive — anchor, end_anchor, and everything between is overwritten.
3. Anchors are file-scoped. "a3${getDelimiter()}" in one file differs from "a3${getDelimiter()}" in another.

Single edit per call. Use ; to chain multiple edits in one turn:
  edit_file src/auth.py --anchor "a3${getDelimiter()}def login():" --content "def login(x): ..." --end-anchor "k7${getDelimiter()}    pass"; edit_file src/db.py --anchor "b2${getDelimiter()}def connect():" --content "def connect() { ... }"

Usage: edit_file <path> --anchor <id> --content <text> [--end-anchor <id>] [--edit-type TYPE]

Positional:
  path                Source file path

Options:
  --anchor ID         (required) Start anchor or insertion point.
  --content TEXT      (required) New text content for the edit.
  --end-anchor ID     End anchor (required for 'replace').
  --edit-type TYPE    Edit type: replace, insert_after, insert_before (default: replace).

Examples:
  edit_file src/auth.py --anchor "a3|def login():" --end-anchor "k7|    pass" --content "def login(x):\\n    ..."
  edit_file src/auth.py --anchor "b2|class Auth:" --content "    def new_method(self):\\n        pass" --edit-type insert_after`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for edit_file.",
			usage: 'src/auth.py --anchor "a3|def login():" --content "def login(x): ..."',
		},
	],
}
