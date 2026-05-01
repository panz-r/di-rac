import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.ATTEMPT

export const attempt_completion: DiracToolSpec = {
	id,
	name: "attempt_completion",
	description: `Presents a summary of the final result to the user. Always provide a clear result message explaining what was done.

Usage: attempt_completion <result> [--command CMD]

Positional:
  result              A clear summary of what was accomplished.

Options:
  --command CMD       Optional CLI command to demo the result (e.g., 'npm test'). Do not use 'echo' or 'cat'.

Examples:
  attempt_completion "Fixed the login bug by updating the auth middleware to validate session tokens"
  attempt_completion "Added caching layer with 10min TTL for user queries" --command "npm test"`,
	parameters: [
		{
			name: "result",
			required: true,
			type: "string",
			instruction: "A clear summary of what was accomplished.",
			usage: '"Fixed the login bug by updating the auth middleware"',
		},
		{
			name: "command",
			required: false,
			type: "string",
			instruction: "Optional CLI command to demo the result.",
			usage: "npm test",
		},
	],
}

export const attempt_completion_variants = [attempt_completion]
