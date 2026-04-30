import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.COMPACT

export const compact: DiracToolSpec = {
	id,
	name: "compact",
	description: `Compress the conversation history into a summary to free context window space. Use this when you sense context pressure or are about to start a new phase of work and old context is no longer needed.

Usage: compact <context> [--required-files PATH...]

Positional:
  context             Comprehensive summary of the conversation so far. Include all technical decisions, code changes made, errors encountered, and current task state. This will be your only context moving forward.

Options:
  --required-files PATH    (repeatable) Relative paths to the most important files needed to continue the task. Up to 8 files will be automatically read back into context.

Examples:
  compact "Refactored auth module. Changed middleware to use JWT. Error: had to update token refresh logic." --required-files src/auth.ts --required-files src/middleware.ts
  compact "Investigated performance issue in query builder. Root cause: N+1 queries in user service."`,
	// Always available — no contextRequirements gating
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for compact.",
			usage: '"Summary here" --required-files src/auth.ts',
		},
	],
}
