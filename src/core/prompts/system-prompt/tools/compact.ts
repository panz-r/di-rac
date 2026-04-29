import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.COMPACT

export const compact: DiracToolSpec = {
	id,
	name: "compact",
	description:
		"Compress the conversation history into a summary to free context window space. Use this when you sense context pressure or are about to start a new phase of work and old context is no longer needed.",
	parameters: [
		{
			name: "context",
			required: true,
			type: "string",
			instruction:
				"Comprehensive summary of the conversation so far. Include all technical decisions, code changes made, errors encountered, and current task state. This will be your only context moving forward.",
		},
		{
			name: "required_files",
			required: false,
			type: "array",
			items: { type: "string" },
			instruction:
				"List of relative paths to the most important files needed to continue the task. Up to 8 files will be automatically read back into context.",
		},
	],
	// Always available — no contextRequirements gating
}
