import type { ToolUse } from "@core/assistant-message"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"

export class SymbolsToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.SYMBOLS

	constructor(private validator: ToolValidator) {}

	getDescription(block: ToolUse): string {
		const params = block.params as Record<string, unknown>
		const sub = (params.subcommand as string) || "search"
		const name = (params.symbol as string) || (params.existing_symbol as string) || (params.query as string) || ""
		const paths = Array.isArray(params.paths) ? (params.paths as string[]).join(" ") : ((params.path as string) || "")
		return `symbols ${sub}${name ? ` ${name}` : ""}${paths ? ` in ${paths}` : ""}`
	}

	async handlePartialBlock(_block: ToolUse, _uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		// No partial handling for symbols
	}

	async execute(_config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const params = block.params as Record<string, unknown>
		const sub = (params.subcommand as string) || "search"
		return `Symbol tool '${sub}' is no longer available.`
	}
}
