import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.COMPACT

export const compact: DiracToolSpec = {
	id,
	name: "compact",
	description: `Compress conversation history. Summary becomes your only context. --keep: file paths to reload (up to 8).

Examples:
  compact "Fixed auth bug. Changed middleware to JWT." --keep src/auth.ts
  compact "Investigated N+1 issue. Root cause found."

Returns: confirmation.
Typical: compact 'Summary of work so far' --keep src/file.ts`,
	contextRequirements: (ctx) => (ctx.toolCallCount ?? 99) >= 5,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			usage: "'Summary here' --keep src/auth.ts",
		},
	],
	metadata: {
		tags: ["compact", "context"],
		category: "context",
		concurrency: "sequential",
		safety: ["read"],
		outputSize: "small",
		llmsBrief: "Compact conversation context",
		compactionSafety: "discardable",
	},
}
