import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.PLAN_MODE

export const plan: DiracToolSpec = {
	id,
	name: "plan",
	description: `Propose a plan. Plan mode only. --explore: more investigation needed.

Examples:
  plan "Refactor auth first, then update tests."
  plan "Need to check database layer." --explore

Response: OK | plan:<text> | tokens:N
	Plan text follows header line.`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for plan mode. Describe your plan or response.",
			usage: "'Plan description here'",
		},
	],
	metadata: {
		tags: ["plan", "propose"],
		category: "interaction",
		concurrency: "sequential",
		safety: ["read"],
		outputSize: "small",
		llmsBrief: "Propose a plan",
		compactionSafety: "discardable",
	},
}
