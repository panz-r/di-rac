import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.SYMBOLS

export const symbols: DiracToolSpec = {
	id,
	name: "symbols",
	description: `Symbol ops. For text/regex patterns, use search instead.
  search --name PATTERN [--kind function|class]    Find defs
  replace --name SYMBOL --text CODE                Replace def
  rename --old NAME --new NAME                     Rename def
  refs --name SYMBOL                               Find refs

Examples:
  symbols search src/ --name AuthService --kind class
  symbols replace src/auth.ts --name login --text "async login() { ... }"
  symbols rename src/ --old calcTotal --new calcGrandTotal
  symbols refs src/ --name login

Returns: up to 50 matches (search). Replace/rename: affected file count.
Typical: symbols search src/ --name AuthService --kind class`,
	contextRequirements: (ctx) => (ctx.toolCallCount ?? 99) >= 5,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
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
