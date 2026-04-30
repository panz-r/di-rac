import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.SEARCH

export const search_files: DiracToolSpec = {
	id,
	name: "search_files",
	description: `Regex search across files in the specified paths (files or directories). Skips non-useful content (.git, node_modules, build artifacts, etc.). Prefer AST tools over this when reasonable.

Usage: search_files <path>... --regex PATTERN [options]

Positional:
  path                Files or directories to search in

Options:
  --regex PATTERN     (required) Rust regex pattern to search for.
  --file-pattern GLOB Glob pattern to filter files (e.g. "*.ts").
  --context-lines N   Context lines before/after each match (0-10, default 0).

Examples:
  search_files src/ --regex "function.*login"
  search_files src/core src/services --regex "export.*class" --file-pattern "*.ts" --context-lines 2`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for search_files.",
			usage: 'src/ --regex "function.*login" --file-pattern "*.ts"',
		},
	],
}
