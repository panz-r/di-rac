import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.RENAME_SYMBOL

export const rename_symbol: DiracToolSpec = {
	id,
	name: "rename_symbol",
	description: `Renames ALL occurrences of a symbol (function, class, method, or variable) inside the specified files or directories. Uses AST-based matching so it understands language structure — more accurate than search-and-replace. Strongly prefer this for renaming tasks.

Usage: rename_symbol <path>... --old <name> --new <name>

Positional:
  path                Files or directories to rename in

Options:
  --old NAME          (required) Current symbol name.
  --new NAME          (required) New symbol name.

Examples:
  rename_symbol src/ tests/ --old calculateTotal --new calculateGrandTotal
  rename_symbol src/auth.ts --old UserService --old AccountService --new UserManager --new AccountManager`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for rename_symbol.",
			usage: 'src/ tests/ --old calculateTotal --new calculateGrandTotal',
		},
	],
}
