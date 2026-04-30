import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.REPLACE_SYMBOL

export const replace_symbol: DiracToolSpec = {
	id,
	name: "replace_symbol",
	description: `Replaces a symbol (function, method, or class) in a file with new code. Targets specific AST nodes directly — more robust and token-efficient than edit_file. You MUST provide the complete replacement including JSDoc, comments, decorators, and export keywords. The tool replaces the entire original range.

Single replacement per call. Use ; to chain multiple replacements in one turn:
  replace_symbol src/auth.py --symbol login --text "def login(x): ..."; replace_symbol src/db.py --symbol connect --text "..."

Usage: replace_symbol <path> --symbol <name> --text <content> [options]

Positional:
  path                Source file path

Options:
  --symbol NAME       (required) Dot-separated path to symbol (e.g. ClassName.methodName).
  --text CONTENT      (required) Complete new code for the symbol.
  --type KIND         Optional symbol type for disambiguation (function, method, class).

Examples:
  replace_symbol src/auth.ts --symbol AuthService.login --text "async login(user: string) { ... }"
  replace_symbol src/db.ts --symbol connect --type function --text "function connect() { ... }"`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for replace_symbol.",
			usage: 'src/auth.ts --symbol AuthService.login --text "async login(user: string) { ... }"',
		},
	],
}
