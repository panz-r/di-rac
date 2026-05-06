import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.SYMBOLS

export const symbols: DiracToolSpec = {
	id,
	name: "symbols",
	description: `Perform AST symbol operations: search definitions, replace bodies, rename across files, find references. For text/regex patterns, use search instead. For reading file content, use read --detail outline --section fn:Name.

Subcommands:
  search --name PATTERN [--kind function|class]    Find definitions
  replace --name SYMBOL --text CODE                Replace definition body
  rename --old NAME --new NAME                     Rename across files
  refs --name SYMBOL                               Find all references

Example: symbols search src/ --name AuthService --kind class

Response: OK | matches:N | hint:Try --kind function/class or different name | tokens:N
Fails when: no matches (typo, wrong --kind), file type not supported by tree-sitter.
If fails: try without --kind, use search for text patterns, check file extension support.
After results: use read --section <handle> to see full body. Use refs to find usages.
Good: definitions found with types and signatures. Bad: no matches (try without --kind or use search), unsupported file type (check extension).
Don't use for: text/regex patterns across files (use search), file overview (use repo).
Output example: OK | matches:2 | tokens:35
  src/auth.ts:10 class AuthService (fn:login, fn:logout)
  src/auth.ts:42 fn login()
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
