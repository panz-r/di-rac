import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.REPO_MAP

export const repo_map: DiracToolSpec = {
	id,
	name: "repo_map",
	description:
		"Provides a lightweight structural summary of the entire repository. Returns a list of files with their key top-level symbols (classes, functions). This is the best tool for initial project orientation and understanding broad architecture.",
	parameters: [],
}
