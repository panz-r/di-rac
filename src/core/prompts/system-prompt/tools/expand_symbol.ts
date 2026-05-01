import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.EXPAND_SYMBOL

export const expand_symbol: DiracToolSpec = {
	id,
	name: "expand_symbol",
	description: `Extracts the complete body of a symbol (function, class, method) using its structural handle (ID). Handles are stable IDs like 'fn:myFunc' or 'class:MyClass' obtained from read_file with --detail outline or skeleton. Most token-efficient way to read specific code blocks.

Usage: expand_symbol <path> --symbol <handle>

Positional:
  path                  Source file path

Options:
  --symbol HANDLE      (required) Structural handle ID of the symbol to expand.

Examples:
  expand_symbol src/auth.ts --symbol fn:AuthService.login
  expand_symbol src/utils/math.ts --symbol class:Calculator`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for expand_symbol.",
			usage: 'src/auth.ts --symbol fn:AuthService.login',
		},
	],
	metadata: {
		tags: ["code", "expand", "symbol"],
		category: "code-intel",
		concurrency: "parallel-safe",
		safety: ["read"],
		outputSize: "medium",
		llmsBrief: "Expand collapsed symbol to show full content",
		compactionSafety: "summarizable",
	},
}
