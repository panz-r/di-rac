import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.COMPACT

export const compact: DiracToolSpec = {
	id,
	name: "compact",
	description: `Compress conversation history. Summary becomes your only context. --keep: file paths to reload (up to 8). Don't compact mid-edit — finish changes first. For reading saved outputs, use memory.

Example: compact "Fixed auth bug. Changed middleware to JWT." --keep src/auth.ts

Response: OK | summary:<text> | reloaded:N | tokens:N
	Summary follows header line.
After results: context is compressed. Use memory to reload key outputs if needed.
Good: summary captures key state, --keep files reloaded. Bad: lost important context (use --keep next time), compacted mid-edit (finish edits first).
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
