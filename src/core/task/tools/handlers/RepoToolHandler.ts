import type { ToolUse } from "@core/assistant-message"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { ListFilesToolHandler } from "./ListFilesToolHandler"

export class RepoToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.LIST_FILES

	constructor(private validator: ToolValidator) {}

	getDescription(block: ToolUse): string {
		const detail = block.params.detail as string
		const paths = Array.isArray(block.params.paths) ? block.params.paths.join(" ") : ""
		return `repo${detail ? ` --detail ${detail}` : ""}${paths ? ` ${paths}` : ""}`
	}

	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		const detail = (block.params.detail as string) || "summary"
		if (detail === "files") {
			return new ListFilesToolHandler(this.validator).handlePartialBlock(block, uiHelpers)
		}
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const detail = (block.params.detail as string) || "summary"

		if (detail === "files") {
			// Delegate to list_files handler, defaulting paths to cwd root
			if (!block.params.paths || (Array.isArray(block.params.paths) && block.params.paths.length === 0)) {
				block = { ...block, params: { ...block.params, paths: ["."] } }
			}
			const handler = new ListFilesToolHandler(this.validator)
			return handler.execute(config, block)
		}

		// repo_map is no longer available
		return `repo tool '${detail}' is no longer available.`
	}
}
