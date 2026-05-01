import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.DIAGNOSTICS_SCAN

export const diagnostics_scan: DiracToolSpec = {
	id,
	name: "diagnostics_scan",
	description: `Runs diagnostics (linter and syntax checks) on the specified files and returns the results. Useful for checking if recent changes introduced errors or for getting a summary of existing problems.

Usage: diagnostics_scan <path>...

Positional:
  path            One or more file paths to scan

Examples:
  diagnostics_scan src/utils/math.ts
  diagnostics_scan src/auth.ts src/db.ts`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for diagnostics_scan.",
			usage: 'src/utils/math.ts src/utils/string.ts',
		},
	],
	metadata: {
		tags: ["diagnostics", "errors", "lsp"],
		category: "code-intel",
		concurrency: "parallel-safe",
		safety: ["read"],
		outputSize: "small",
		llmsBrief: "Scan for diagnostics, errors, and warnings",
		compactionSafety: "summarizable",
	},
}
