import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.BASH

export const bash: DiracToolSpec = {
	id,
	name: "bash",
	description: `Execute shell commands. Composition (pipes, &&, ||) is encouraged to minimize round-trips. Use heredocs for multi-line scripts.

Examples:
  bash "git diff --cached"
  bash "grep -r 'TODO' src/ | wc -l"
  bash "npm test && npm run build"
  bash "python3 << 'EOF'
import os
print(os.getcwd())
EOF"

Returns: JSON {ok, exitCode, stdout, stderr}. Output truncated at 8KB.
Typical: bash 'npm test && npm run build'`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "The full shell command to execute. Pipes, &&, ||, heredocs, and subshells all work.",
			usage: '"npm test && npm run build"',
		},
	],
	metadata: {
		tags: ["shell", "execution"],
		category: "execution",
		concurrency: "sequential",
		safety: ["destructive", "network"],
		supportsForce: true,
		outputSize: "large",
		llmsBrief: "Execute shell commands with full bash support",
		compactionSafety: "discardable",
	},
}
