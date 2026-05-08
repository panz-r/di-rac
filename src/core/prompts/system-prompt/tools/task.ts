import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.NEW_TASK

export const task: DiracToolSpec = {
	id,
	name: "task",
	description: `Create a new task with preloaded context. Use for major context switches. The new task starts with ONLY the context you provide — make it self-contained with work summary, key files, progress, and next steps.

Example: task "Refactoring auth. Done: extracted middleware (auth.ts). Next: token refresh tests, login flow update. Key files: src/auth.ts, src/middleware.ts"

Response: OK | task_id:<id> | tokens:N
	After results: new task created. Start working in it. Use compact to save current context if needed.`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for task. Use --prompt to start a subtask.",
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
