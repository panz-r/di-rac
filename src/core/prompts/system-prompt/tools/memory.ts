import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.DIRAC_OUTPUTS

export const memory: DiracToolSpec = {
	id,
	name: "memory",
	description: `Manage saved tool outputs. No args or --list: list files. Filename: read file. --clear: delete all. Use to preserve outputs across compactions.

Example: memory output.txt

Response: OK | items:N | <list> | tokens:N
Don't use for: current code (use read/search), temporary data (use bash temp files).
Typical: memory --list`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for memory. Use --list to list files, filename to read, --clear to delete.",
			usage: "--clear",
		},
	],
	metadata: {
		tags: ["outputs", "files"],
		category: "meta",
		concurrency: "parallel-safe",
		safety: ["read"],
		outputSize: "small",
		llmsBrief: "Manage saved outputs",
		compactionSafety: "discardable",
	},
}
