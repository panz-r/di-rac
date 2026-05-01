import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.PLAN_MODE

export const plan_mode_respond: DiracToolSpec = {
	id,
	name: "plan_mode_respond",
	description: `Proposes a step-by-step solution plan to the user. Use only in PLAN MODE after exploring the codebase.

Usage: plan_mode_respond <response> [--needs-more-exploration]

Positional:
  response            The response to provide to the user.

Options:
  --needs-more-exploration    Set if more exploration is required.

Examples:
  plan_mode_respond "I recommend refactoring the auth module first, then updating the tests."
  plan_mode_respond "Need to investigate the database layer further." --needs-more-exploration`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for plan_mode_respond.",
			usage: '"Your plan here" --needs-more-exploration',
		},
	],
	metadata: {
		tags: ["plan", "mode", "interactive"],
		category: "interaction",
		concurrency: "sequential",
		safety: ["interactive"],
		outputSize: "small",
		llmsBrief: "Respond during plan mode interaction",
		compactionSafety: "discardable",
	},
}
