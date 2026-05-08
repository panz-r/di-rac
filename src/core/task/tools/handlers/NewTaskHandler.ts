import type { ToolUse } from "@core/assistant-message"
import { formatResponse } from "@core/prompts/responses"

import { processFilesIntoText } from "@integrations/misc/extract-text"
import { showSystemNotification } from "@integrations/notifications"
import { DiracDefaultTool } from "@/shared/tools"
import { createToolError } from "@shared/tool-response"
import type { ToolResponse } from "../../index"
import type { IPartialBlockHandler, IToolHandler } from "../ToolExecutorCoordinator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"

export class NewTaskHandler implements IToolHandler, IPartialBlockHandler {
	readonly name = DiracDefaultTool.NEW_TASK
	constructor() {}

	getDescription(block: ToolUse): string {
		return `[${block.name} for creating a new task]`
	}

	/**
	 * Handle partial block streaming for new_task
	 */
	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		const context = uiHelpers.removeClosingTag(block, "context", block.params.context)
		if (!context) {
			return
		}

		await uiHelpers.ask("new_task", context, true).catch(() => {})
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const context: string | undefined = block.params.context

		// Validate required parameters
		if (!context) {
			config.taskState.consecutiveMistakeCount++
			const hint = "Missing context for task. Provide a 'command' argument with a self-contained summary.\n"
				+ 'Example: use task with command="Refactoring auth middleware. Key files: src/auth.ts, src/middleware.ts. Done: extracted token validation. Next: update session storage. Blockers: none."'
			await config.callbacks.say("error", "di tried to create a new task without context. Retrying...")
			return formatResponse.formatToolErrorForLLM(createToolError("tool.invalidInput", hint, "recoverable"))
		}

		// Reject overly brief context — the new task needs enough detail to continue
		if (context.trim().length < 100) {
			config.taskState.consecutiveMistakeCount++
			const msg = `Task context too brief (${context.trim().length} chars). Provide a comprehensive summary including: current work, key files, what's done, what's next, any blockers. The new task starts with ONLY this context — make it self-contained.`
			await config.callbacks.say("error", msg)
			return formatResponse.formatToolErrorForLLM(createToolError("tool.invalidInput", msg, "recoverable"))
		}

		config.taskState.consecutiveMistakeCount = 0

		// Show notification if enabled
		if (config.autoApprovalSettings.enableNotifications) {
			showSystemNotification({
				subtitle: "di wants to start a new task...",
				message: `di is suggesting to start a new task with: ${context}`,
			})
		}

		// Ask user for response
		await config.callbacks.removeLastPartialMessageIfExistsWithType("ask", this.name as any)
		const { text, images, files: newTaskFiles } = await config.callbacks.ask("new_task", context, false)

		// If the user provided a response, treat it as feedback
		if (text || (images && images.length > 0) || (newTaskFiles && newTaskFiles.length > 0)) {
			let fileContentString = ""
			if (newTaskFiles && newTaskFiles.length > 0) {
				fileContentString = await processFilesIntoText(newTaskFiles)
			}

			await config.callbacks.say("user_feedback", text ?? "", images, newTaskFiles)
			const apiConfig = config.services.stateManager.getApiConfiguration()
			const provider = (config.mode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string


			return formatResponse.toolResult(
				`The user provided feedback instead of creating a new task:
<feedback>
${text}
</feedback>`,

				images,
				fileContentString,
			)


		}
		// If no response, the user clicked the "Create New Task" button
		const apiConfig = config.services.stateManager.getApiConfiguration()
		const provider = (config.mode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string



		return formatResponse.toolResult(`The user has created a new task with the provided context.`)
	}
}
