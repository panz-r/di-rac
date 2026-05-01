import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.ASK

export const ask_followup_question: DiracToolSpec = {
	id,
	name: "ask_followup_question",
	description: `Asks the user a clarifying question when you encounter ambiguities or need more details.

Usage: ask_followup_question <question> [--options JSON]

Positional:
  question            The question to ask the user.

Options:
  --options JSON      Optional JSON array of 2-5 predefined answer options. DO NOT include options to toggle Act mode.

Examples:
  ask_followup_question "Should we use JWT or session-based auth?" --options '["JWT", "Session-based", "OAuth"]'
  ask_followup_question "Which database should we target?"`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for ask_followup_question.",
			usage: '"Should we use X or Y?" --options \'["Option 1", "Option 2"]\'',
		},
	],
	metadata: {
		tags: ["interactive", "question", "user"],
		category: "interaction",
		concurrency: "sequential",
		safety: ["interactive"],
		outputSize: "small",
		llmsBrief: "Ask user a followup question with options",
		compactionSafety: "essential",
	},
}

export const ask_followup_question_variants = [ask_followup_question]
