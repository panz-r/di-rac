import type { ToolUse } from "@core/assistant-message"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { SearchSymbolsToolHandler } from "./SearchSymbolsToolHandler"
import { ReplaceSymbolToolHandler } from "./ReplaceSymbolToolHandler"
import { RenameSymbolToolHandler } from "./RenameSymbolToolHandler"
import { FindSymbolReferencesToolHandler } from "./FindSymbolReferencesToolHandler"

export class SymbolsToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.SYMBOLS

	constructor(private validator: ToolValidator) {}

	getDescription(block: ToolUse): string {
		const sub = (block.params.subcommand as string) || "search"
		const name = block.params.symbol || block.params.existing_symbol || block.params.query || block.params.symbols?.[0] || ""
		const paths = Array.isArray(block.params.paths) ? block.params.paths.join(" ") : (block.params.path || "")
		return `symbols ${sub}${name ? ` ${name}` : ""}${paths ? ` in ${paths}` : ""}`
	}

	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		// Delegate to sub-handler for partial UI
		const sub = (block.params.subcommand as string) || "search"
		const handler = this.getSubHandler(sub)
		if (handler && "handlePartialBlock" in handler) {
			return (handler as any).handlePartialBlock(block, uiHelpers)
		}
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const sub = (block.params.subcommand as string) || "search"
		const handler = this.getSubHandler(sub)
		if (!handler) {
			return `Unknown symbols subcommand: "${sub}". Use: search, replace, rename, refs`
		}
		return handler.execute(config, block)
	}

	private getSubHandler(sub: string): IFullyManagedTool | null {
		switch (sub) {
			case "search": return new SearchSymbolsToolHandler(this.validator)
			case "replace": return new ReplaceSymbolToolHandler(this.validator)
			case "rename": return new RenameSymbolToolHandler(this.validator)
			case "refs": return new FindSymbolReferencesToolHandler(this.validator)
			default: return null
		}
	}
}
