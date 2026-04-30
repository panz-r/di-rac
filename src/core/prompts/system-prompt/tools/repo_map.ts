import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.REPO_MAP

export const repo_map: DiracToolSpec = {
	id,
	name: "repo_map",
	description: `Provides a lightweight structural summary of the entire repository. Returns a list of files with their key top-level symbols (classes, functions). Best tool for initial project orientation and understanding broad architecture.

Usage: repo_map

No arguments required.`,
	parameters: [
		{
			name: "command",
			required: false,
			type: "string",
			instruction: "No arguments needed. Pass empty string or omit.",
		},
	],
}
