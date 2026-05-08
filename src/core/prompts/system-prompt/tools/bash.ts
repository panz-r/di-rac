import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.BASH

export const bash: DiracToolSpec = {
	id,
	name: "bash",
	description: `Execute shell commands. Composition (pipes, &&, ||) is encouraged to minimize round-trips. Use heredocs for multi-line scripts. Dangerous commands (recursive deletes, reverse shells) are blocked; blocked will name the pattern. Don't edit files with bash — use edit. Don't read files — use read.

	Example: bash "npm test && npm run build"

	Long-running commands: if a command exceeds the turn delay (~10s), it returns partial output with "exit:running" and a command ID. The command continues in the background. Retrieve the final result later with: bash --await <id>. This lets you do other work while waiting.

	Response: OK | tokens:N | lines:N | exit:N followed by stdout. [stderr], [truncated], [timed out], [blocked:pattern], [security:violation] appended as applicable.
	Note: stdout truncated at ~8KB, stderr at ~2KB (head+tail preserved). Use redirects to file for larger output.
	Fails when: timeout (>300s default), exit≠0 (check stderr), output truncated, blocked:pattern.
	If fails: --timeout 60 for slow commands; redirect large output to file; blocked shows the pattern.
	After results: check exit code. If non-zero, read stderr. If truncated, redirect to file then read.
	Good: exit:0 with expected output visible. Bad: exit!=0 (read stderr), truncated (redirect to file), timed_out (use --timeout).
	Output example: exit:0
	  src/auth.ts  42 | a3|def login():
	  src/auth.ts  58 | k7|  return token
	Running example: exit:running [Command still running — 12s elapsed, 4500 bytes output] Recent output: ...bash --await 42
	Universal flags: --timeout N (max seconds to wait, default 300s, max 600s), --retry N (retry on error, max 5), --await <id> (retrieve result of background command).
	Typical: bash 'npm test && npm run build'`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "The full shell command to execute. Pipes, &&, ||, heredocs, and subshells all work. Use --await <id> to retrieve a background command's result.",
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
		llmsBrief: "Execute shell commands with full bash support. Long commands return partial results with --await for later retrieval.",
		compactionSafety: "discardable",
	},
}
