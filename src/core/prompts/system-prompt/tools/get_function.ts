import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.GET_FUNCTION

export const get_function: DiracToolSpec = {
	id,
	name: "get_function",
	description: `Extracts the complete implementation of one or more functions or methods from one or more files. Use this to inspect specific functions' logic without reading entire files. Supports all-to-all lookup across multiple files and function names. Use dot-separated paths for nested symbols.

Usage: get_function <path>... --fn <name>...

Positional:
  path            One or more source file paths to search in

Options:
  --fn NAME       (required, repeatable) Function or method name to extract.
                  Use dot-separated paths like ClassName.methodName.

Examples:
  get_function src/utils/math.ts --fn calculateSum
  get_function src/auth.ts src/db.ts --fn AuthService.login --fn connectDB`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for get_function.",
			usage: 'src/utils/math.ts --fn calculateSum --fn findMax',
		},
	],
	metadata: {
		tags: ["code", "function", "read"],
		category: "code-intel",
		concurrency: "parallel-safe",
		safety: ["read"],
		outputSize: "medium",
		llmsBrief: "Extract function bodies by name from source files",
		compactionSafety: "summarizable",
	},
}
