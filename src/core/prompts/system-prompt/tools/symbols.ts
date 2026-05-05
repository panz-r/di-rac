import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.SYMBOLS

export const symbols: DiracToolSpec = {
	id,
	name: "symbols",
	description: `AST symbol operations. For text/regex patterns, use search instead.

Subcommands:
  search --name PATTERN [--kind function|class]    Find definitions
  replace --name SYMBOL --text CODE                Replace definition body
  rename --old NAME --new NAME                     Rename across files
  refs --name SYMBOL                               Find all references

Examples:
  symbols search src/ --name AuthService --kind class
  symbols replace src/auth.ts --name login --text "async login() { ... }"
  symbols rename src/ --old calcTotal --new calcGrandTotal
  symbols refs src/ --name login

Response: OK | matches:N | hint:Try --kind function/class or different name | tokens:N
Universal flags: --dry-run (preview changes without applying), --retry N.
Typical: symbols search src/ --name AuthService --kind class`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for symbols. Subcommands: search, replace, rename.",
			usage: "replace src/auth.ts --name login --text '...'",
		},
	],
	metadata: {
		tags: ["ast", "symbol", "refactor"],
		category: "code",
		concurrency: "sequential",
		safety: ["read", "write"],
		outputSize: "medium",
		llmsBrief: "AST symbol operations",
		compactionSafety: "discardable",
	},
}
