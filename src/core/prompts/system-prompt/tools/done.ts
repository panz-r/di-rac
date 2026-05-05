import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.ATTEMPT

export const done: DiracToolSpec = {
	id,
	name: "done",
	description: `Mark task complete with result summary. --cmd: optional demo command (not echo/cat).

Examples:
  done "Fixed auth bug by updating middleware"
  done "Added caching layer" --cmd "npm test"

Returns: confirmation + optional command output.
Typical: done 'Fixed the bug'`,
		contextRequirements: (ctx) => (ctx.toolCallCount ?? 99) >= 15,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
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
