import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.DIRAC_RECALL

export const recall: DiracToolSpec = {
	id,
	name: "recall",
	description: `Search archived conversation observations from past tasks. Returns matches ranked by relevance. Results may be stale — always verify against current code.

Example: recall auth middleware

Response: OK | matches:N | tokens:N
Note: Results are from past task observations, not current codebase. Verify before acting.
Good: relevant past observations found. Bad: stale results (verify with read/search), no matches (broaden query).
Don't use for: current code state (use read/search), web info (use web_search).
Typical: recall auth middleware`,
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
