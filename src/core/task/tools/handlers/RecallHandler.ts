import type { ToolUse } from "@core/assistant-message"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import type { TaskConfig } from "../types/TaskConfig"
import { IToolHandler } from "../ToolExecutorCoordinator"

export class RecallHandler implements IToolHandler {
	readonly name = DiracDefaultTool.DIRAC_RECALL

	getDescription(block: ToolUse): string {
		const query = block.params.query as string | undefined
		return `[dirac_recall: ${query ?? "all"}]`
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const observer = config.services.observerOrchestrator
		if (!observer?.isEnabled) {
			return "Observer is not enabled. Enable observer in settings to use recall."
		}

		const query = (block.params.query as string)?.trim()
		if (!query) {
			return "Usage: dirac_recall <query> — provide a keyword to search observations."
		}

		return observer.recall(query)
	}
}
