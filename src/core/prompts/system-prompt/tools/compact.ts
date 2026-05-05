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

Response: OK | summary:<text> | reloaded:N | tokens:N
	Summary follows header line.
Typical: compact 'Summary of work so far' --keep src/file.ts`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for compact. Summarizes conversation context.",
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
