import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.DIRAC_RECALL

export const recall: DiracToolSpec = {
	id,
	name: "recall",
	description: `Search archived conversation observations.

Examples:
  recall auth middleware
  recall error in login

Response: OK | matches:N | tokens:N
Note: Results are from past task observations, not current codebase.`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
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
