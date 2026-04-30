import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.NEW_TASK

export const new_task: DiracToolSpec = {
	id,
	name: "new_task",
	description: `Creates a new task with preloaded context from the current conversation.

Usage: new_task <context>

Positional:
  context             Detailed summary of the conversation so far, including current work, technical concepts, modified files, problems solved, and exact pending next steps.

Examples:
  new_task "Refactoring the auth module. Completed: middleware update. Remaining: update tests and add integration test."`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for new_task.",
			usage: '"Detailed conversation summary here"',
		},
	],
}
