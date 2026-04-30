import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.DIRAC_RECALL

export const recall: DiracToolSpec = {
	id,
	name: "dirac_recall",
	description: `Search archived conversation observations.

Usage: dirac_recall <query>

Positional:
  query            Keyword or phrase to search observation history

Examples:
  dirac_recall auth middleware decision
  dirac_recall error in login
  dirac_recall files modified`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for dirac_recall.",
			usage: "auth middleware",
		},
	],
	metadata: {
		tags: ["meta", "memory", "search"],
		category: "meta",
		concurrency: "parallel-safe",
		safety: ["read"],
		outputSize: "medium",
		llmsBrief: "Search archived conversation observations by keyword",
		compactionSafety: "discardable",
	},
}
