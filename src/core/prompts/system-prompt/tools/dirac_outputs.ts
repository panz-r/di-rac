import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.DIRAC_OUTPUTS

export const dirac_outputs: DiracToolSpec = {
	id,
	name: "dirac_outputs",
	description: `Manage saved tool outputs.

Usage: dirac_outputs [file] [options]

Positional:
  file            Optional filename to read from .dirac/outputs/

Options:
  --clear         Delete all saved output files

Examples:
  dirac_outputs
  dirac_outputs read_file_2026-05-01T12-00-00.txt
  dirac_outputs --clear`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for dirac_outputs.",
			usage: "--clear",
		},
	],
	metadata: {
		tags: ["meta", "output", "files"],
		category: "meta",
		concurrency: "sequential",
		safety: ["read", "write"],
		outputSize: "medium",
		llmsBrief: "List, read, or clear saved tool outputs from .dirac/outputs",
		compactionSafety: "discardable",
	},
}
