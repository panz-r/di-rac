import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.USE_SUBAGENTS

export const subagent: DiracToolSpec = {
	id,
	name: "use_subagents",
	description: `Run between two and five focused in-process subagents in parallel. Each subagent gets its own prompt and returns a comprehensive research result. Default timeout is 300 seconds. Particularly effective for investigating multiple independent paths simultaneously without consuming your context window.

Usage: use_subagents --prompt TEXT --prompt TEXT [--prompt TEXT] [--include-history] [--timeout SEC] [--max-turns N]

Options:
  --prompt TEXT         (required, repeat 2-5 times) Subagent prompt.
  --include-history     Include the main task's conversation history.
  --timeout SEC         Timeout per subagent in seconds (default: 300).
  --max-turns N         Maximum number of turns per subagent.

Examples:
  use_subagents --prompt "Research auth patterns in the codebase" --prompt "Check test coverage for auth module"
  use_subagents --prompt "Find all API endpoints" --prompt "Check for security issues" --prompt "Review error handling" --timeout 120

Response: OK | results:N | turns:N | tokens:N
	Results follow: prompt | summary | tools_used (one per subagent).
Typical: use_subagents --prompt "Research auth patterns" --prompt "Check test coverage"`,
	contextRequirements: (ctx) => ctx.subagentsEnabled === true,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for use_subagents.",
			usage: '--prompt "Research auth patterns" --prompt "Check test coverage"',
		},
	],
	metadata: {
		tags: ["subagent", "delegate", "parallel"],
		category: "execution",
		concurrency: "sequential",
		safety: ["read", "write", "destructive", "network"],
		outputSize: "large",
		llmsBrief: "Delegate work to subagent instances",
		compactionSafety: "discardable",
	},
}
