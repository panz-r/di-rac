import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.GET_FILE_SKELETON

export const get_file_skeleton: DiracToolSpec = {
	id,
	name: "get_file_skeleton",
	description: `Reads the structural outline of one or more files by extracting lines where classes, functions, and methods are defined (including nested definitions) while stripping all implementation logic. Use this to quickly understand multiple files' structures and APIs before requesting specific functions.

Usage: get_file_skeleton <path>...

Positional:
  path            One or more source file paths

Examples:
  get_file_skeleton src/utils/math.ts
  get_file_skeleton src/auth.ts src/db.ts src/config.ts`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for get_file_skeleton.",
			usage: 'src/utils/math.ts src/utils/string.py',
		},
	],
}
