import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.PLAN_MODE

export const plan: DiracToolSpec = {
	id,
	name: "plan",
	description: `Propose a plan. Plan mode only. --explore: more investigation needed.

Example: plan "Refactor auth first, then update tests."

Response: OK | plan:<text> | tokens:N
	Plan text follows header line.
	After results: wait for user approval. If approved, start executing first step.
	Good: clear steps with dependencies. Bad: vague plan (add specifics), missing edge cases (consider error paths).`,
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
