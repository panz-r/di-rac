import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.BASH

export const bash: DiracToolSpec = {
	id,
	name: "bash",
	description: `Execute shell commands. Composition (pipes, &&, ||) is encouraged to minimize round-trips. Use heredocs for multi-line scripts. Dangerous commands (recursive deletes, reverse shells) are blocked; blocked will name the pattern.

Examples:
  bash "git diff --cached"
  bash "grep -r 'TODO' src/ | wc -l"
  bash "npm test && npm run build"
  bash "python3 << 'EOF'
import os
print(os.getcwd())
EOF"

Response: OK | tokens:N | lines:N | exit:N followed by stdout. [stderr], [truncated], [timed out], [blocked:pattern], [security:violation] appended as applicable.
Note: stdout truncated at ~8KB, stderr at ~2KB (head+tail preserved). Use redirects to file for larger output.
Universal flags: --dry-run (preview without executing), --retry N (retry on error, max 5).
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
