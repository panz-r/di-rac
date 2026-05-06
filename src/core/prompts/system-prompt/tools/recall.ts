import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.DIRAC_RECALL

export const recall: DiracToolSpec = {
	id,
	name: "recall",
	description: `Search archived conversation observations.

Example: recall auth middleware

Response: OK | matches:N | tokens:N
Note: Results are from past task observations, not current codebase.`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for recall. Use --query to search past observations.",
			usage: "auth middleware",
		},
	],
	metadata: {
		tags: ["meta", "memory", "search"],
		category: "meta",
		concurrency: "parallel-safe",
		safety: ["read"],
		outputSize: "medium",
		llmsBrief: "Search archived observations",
		compactionSafety: "discardable",
	},
}
