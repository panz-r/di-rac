import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.BASH

export const bash: DiracToolSpec = {
	id,
	name: "bash",
	description: `Execute shell commands. Composition (pipes, &&, ||) is encouraged to minimize round-trips. Use heredocs for multi-line scripts. Dangerous commands (recursive deletes, reverse shells) are blocked; blocked will name the pattern. Don't edit files with bash — use edit. Don't read files — use read.

Example: bash "npm test && npm run build"

Response: OK | tokens:N | lines:N | exit:N followed by stdout. [stderr], [truncated], [timed out], [blocked:pattern], [security:violation] appended as applicable.
Note: stdout truncated at ~8KB, stderr at ~2KB (head+tail preserved). Use redirects to file for larger output.
Universal flags: --timeout N (max seconds to wait, default 30s, max 600s), --retry N (retry on error, max 5).
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
