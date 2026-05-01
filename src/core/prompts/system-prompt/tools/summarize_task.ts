import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.SUMMARIZE_TASK

export const summarize_task: DiracToolSpec = {
	id,
	name: "summarize_task",
	description: `Summarize the task to free up context window space.

Usage: summarize_task <context> [--required-files PATH...]

Positional:
  context             Detailed summary of the conversation so far, including current work, technical concepts, modified files, problems solved, and exact pending next steps.

Options:
  --required-files PATH    (repeatable) Relative paths to the most important files needed to continue the task.

Examples:
  summarize_task "Fixed auth middleware. Next: update tests." --required-files src/auth.ts --required-files tests/auth.test.ts
  summarize_task "Refactoring database layer. See pending changes in db module."`,
	contextRequirements: (context) => context.shouldCompact === true,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for summarize_task.",
			usage: '"Summary here" --required-files src/auth.ts --required-files src/db.ts',
		},
	],
	metadata: {
		tags: ["summarize", "task", "context"],
		category: "context",
		concurrency: "sequential",
		safety: ["read"],
		outputSize: "small",
		llmsBrief: "Summarize current task state",
		compactionSafety: "discardable",
	},
}
