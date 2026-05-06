import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.ATTEMPT

export const done: DiracToolSpec = {
	id,
	name: "done",
	description: `Mark task complete with result summary. --cmd: optional demo command (not echo/cat).

Example: done "Added caching layer" --cmd "npm test"

Response: OK | summary:<text> | tokens:N
	Summary follows header line.
Typical: done 'Fixed the bug'`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "Summary of what was accomplished and the final result.",
			usage: "'Fixed auth bug' --cmd 'npm test'",
		},
	],
	metadata: {
		tags: ["completion", "done"],
		category: "interaction",
		concurrency: "sequential",
		safety: ["read"],
		outputSize: "small",
		llmsBrief: "Mark task complete",
		compactionSafety: "discardable",
	},
}
