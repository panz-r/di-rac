import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.USE_SKILL

export const use_skill: DiracToolSpec = {
	id,
	name: "use_skill",
	description: `Load and activate a skill by name. Skills provide specialized instructions for specific tasks. Use this tool ONCE when a user's request matches one of the available skill descriptions shown in the SKILLS section of your system prompt. After activation, follow the skill's instructions directly - do not call use_skill again.

Usage: use_skill <skill_name>

Positional:
  skill_name          The name of the skill to activate (must match exactly one of the available skill names).

Example: use_skill react-component

Response: OK | skill:<name> | tokens:N
	Skill instructions follow header line.`,
	contextRequirements: (ctx) => ctx.skills !== undefined && ctx.skills.length > 0,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for use_skill.",
			usage: "skill_name",
		},
	],
	metadata: {
		tags: ["skill", "invoke", "plugin"],
		category: "meta",
		concurrency: "sequential",
		safety: ["read", "write", "destructive"],
		outputSize: "medium",
		llmsBrief: "Invoke a named skill",
		compactionSafety: "discardable",
	},
}
