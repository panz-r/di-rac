import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.ATTEMPT

export const attempt_completion: DiracToolSpec = {
	id,
	name: "attempt_completion",
	description: `Presents a brief and informative summary of the final result. Keep it concise while covering important changes.

Usage: attempt_completion <result> [--command CMD]

Positional:
  result              The final result of the task.

Options:
  --command CMD       Optional CLI command to demo the result (e.g., 'open index.html'). Do not use 'echo' or 'cat'.

Examples:
  attempt_completion "Fixed the login bug by updating the auth middleware"
  attempt_completion "Added caching layer" --command "npm test"`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for attempt_completion.",
			usage: '"I have completed the task..." --command "open index.html"',
		},
	],
}

export const attempt_completion_variants = [attempt_completion]
