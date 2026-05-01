import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.BASH_RESTRICTED

export const bash: DiracToolSpec = {
	id,
	name: "bash",
	description:
		"Executes a composed shell command in a restricted environment (rbash). Composition like 'grep | head' or 'find | xargs grep' is encouraged to minimize round-trips. Always use absolute or relative paths within the project root. Composition is much more token-efficient than multiple specialized tool calls.",
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "The full shell command to execute. Only allowed binaries are: git, grep, find, cat, head, tail, jq, wc, sort, uniq, curl, sed, awk, python3, node, ls.",
			usage: '"grep -r \'function login\' src/ | head -n 20"',
		},
	],
	metadata: {
		tags: ["shell", "restricted", "sandboxed"],
		category: "execution",
		concurrency: "sequential",
		safety: ["read", "write"],
		outputSize: "medium",
		llmsBrief: "Execute restricted/sandboxed bash commands",
		compactionSafety: "discardable",
	},
}
