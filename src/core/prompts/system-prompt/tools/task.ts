import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.NEW_TASK

export const task: DiracToolSpec = {
	id,
	name: "task",
	description: `Create a new task with preloaded context. Use for major context switches.

Examples:
  task "Refactoring auth. Done: middleware. Next: tests."`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			usage: "'Context summary here'",
		},
	],
	metadata: {
		tags: ["task", "context"],
		category: "lifecycle",
		concurrency: "sequential",
		safety: ["read"],
		outputSize: "small",
		llmsBrief: "Create new task with context",
		compactionSafety: "discardable",
	},
}
