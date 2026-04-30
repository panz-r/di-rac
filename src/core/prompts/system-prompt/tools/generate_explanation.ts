import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.GENERATE_EXPLANATION

export const generate_explanation: DiracToolSpec = {
	id,
	name: "generate_explanation",
	description: `Opens a multi-file diff view and generates AI-powered inline comments explaining the changes between two git references. Use this tool to help users understand code changes from git commits, pull requests, branches, or any git refs. The tool uses git to retrieve file contents and displays a side-by-side diff view with explanatory comments.

Usage: generate_explanation <title> --from-ref <ref> [--to-ref <ref>]

Positional:
  title               A descriptive title for the diff view.

Options:
  --from-ref REF      (required) Git reference for the 'before' state (commit hash, branch, tag, HEAD~1, etc.).
  --to-ref REF        Git reference for the 'after' state. If not provided, compares to working directory.

Examples:
  generate_explanation "Changes in last commit" --from-ref HEAD~1
  generate_explanation "PR #42: Add authentication" --from-ref origin/main --to-ref feature/auth
  generate_explanation "Staging area changes" --from-ref HEAD`,
	contextRequirements: (context) => context.isCliEnvironment !== true,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for generate_explanation.",
			usage: '"Changes in last commit" --from-ref HEAD~1',
		},
	],
}
