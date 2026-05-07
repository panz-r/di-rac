import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.ASK

export const ask: DiracToolSpec = {
	id,
	name: "ask",
	description: `Ask user for clarification. --options: comma-separated choices (2-5).

Example: ask "JWT or session?" --options JWT,Session,OAuth

Response: OK | <user_response> | tokens:N
	Good: clear answer with one of the options. Bad: ambiguous response (ask again with narrower options).
Typical: ask 'Which approach?' --options A,B,C`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for ask. Use --question and optional --options.",
			usage: "'JWT or session?' --options JWT,Session,OAuth",
		},
	],
	metadata: {
		tags: ["question", "user"],
		category: "interaction",
		concurrency: "sequential",
		safety: ["read"],
		outputSize: "small",
		llmsBrief: "Ask user a question",
		compactionSafety: "discardable",
	},
}
