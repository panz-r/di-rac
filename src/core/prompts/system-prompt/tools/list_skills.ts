import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.LIST_SKILLS

export const list_skills: DiracToolSpec = {
	id,
	name: "list_skills",
	description: `List all available skills and their descriptions. Use this to discover specialized capabilities when the initial list in the system prompt is truncated.

Usage: list_skills

No arguments required.`,
	parameters: [
		{
			name: "command",
			required: false,
			type: "string",
			instruction: "No arguments needed. Pass empty string or omit.",
		},
	],
	contextRequirements: (context) => context.skills !== undefined && context.skills.length > 0,
	metadata: {
		tags: ["skill", "list", "discovery"],
		category: "meta",
		concurrency: "parallel-safe",
		safety: ["read"],
		outputSize: "small",
		llmsBrief: "List available skills",
		compactionSafety: "discardable",
	},
}
