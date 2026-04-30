import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

export const execute_command: DiracToolSpec = {
	id: DiracDefaultTool.BASH,
	name: "execute_command",
	description: `Executes CLI commands or scripts. Use the positional form for shell commands (supports pipes, &&, ||, etc.). Use --script for multi-line logic or non-shell languages like Python or Node. Scripts have full filesystem access.

Usage: execute_command <command> [options]

Positional:
  command             Shell command string. Operators like &&, ||, ;, | work natively.

Options:
  --script TEXT       Multi-line script to execute (use instead of positional command).
  --language LANG     Script language: bash (default), python, node.

Provide exactly one of: positional command or --script.

Examples:
  execute_command "npm test && npm run build"
  execute_command "grep -r 'TODO' src/ | wc -l"
  execute_command --script "import os\\nprint(os.getcwd())" --language python`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for execute_command.",
			usage: '"npm test && npm run build"',
		},
	],
}
