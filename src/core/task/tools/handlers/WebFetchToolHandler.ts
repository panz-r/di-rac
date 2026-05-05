import { DiracSayTool } from "@shared/ExtensionMessage"
import { DiracDefaultTool } from "@shared/tools"
import { createToolError } from "@shared/tool-response"
import axios from "axios"
import { DiracEnv } from "@/config"
import { buildDiracExtraHeaders } from "@/services/EnvUtils"
import { DIRAC_ACCOUNT_AUTH_ERROR_MESSAGE } from "@/shared/DiracAccount"
import { getAxiosSettings } from "@/shared/net"
import { ToolUse } from "../../../assistant-message"
import { formatResponse } from "../../../prompts/responses"
import { ToolResponse } from "../.."
import { showNotificationForApproval } from "../../utils"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { ToolResultUtils } from "../utils/ToolResultUtils"

export class WebFetchToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.WEB_FETCH

	getDescription(block: ToolUse): string {
		return `[${block.name} for '${block.params.url}']`
	}

		async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		const url = block.params.url || ""
		const sharedMessageProps: DiracSayTool = {
			tool: "webFetch",
			path: uiHelpers.removeClosingTag(block, "url", url),
			content: `Fetching URL: ${uiHelpers.removeClosingTag(block, "url", url)}`,
			operationIsLocatedInWorkspace: false, // web_fetch is always external
		} satisfies DiracSayTool

		const partialMessage = JSON.stringify(sharedMessageProps)

		if (await uiHelpers.shouldAutoApproveTool(this.name)) {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("ask", "tool")
			await uiHelpers.say("tool", partialMessage, undefined, undefined, block.partial)
		} else {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("say", "tool")
			await uiHelpers.ask("tool", partialMessage, block.partial).catch(() => {})
		}
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		try {
			const url: string | undefined = block.params.url
			const prompt: string | undefined = block.params.prompt

			// Extract provider information for telemetry
			const apiConfig = config.services.stateManager.getApiConfiguration()
			const currentMode = config.services.stateManager.getGlobalSettingsKey("mode")
			const provider = (currentMode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string

			// Check if Dirac web tools are enabled
			const diracWebToolsEnabled = config.services.stateManager.getGlobalSettingsKey("diracWebToolsEnabled")
			if (provider !== "dirac" || !diracWebToolsEnabled) {
				return formatResponse.formatToolErrorForLLM(createToolError("tool.unknownError", "Web tools are currently disabled.", "unrecoverable"))
			}

			// Validate required parameters
			if (!url) {
				config.taskState.consecutiveMistakeCount++
				return await config.callbacks.sayAndCreateMissingParamError(this.name, "url")
			}
			if (!prompt) {
				config.taskState.consecutiveMistakeCount++
				return await config.callbacks.sayAndCreateMissingParamError(this.name, "prompt")
			}
			config.taskState.consecutiveMistakeCount = 0

			// Create message for approval
			const sharedMessageProps: DiracSayTool = {
				tool: "webFetch",
				path: url,
				content: `Fetching URL: ${url}`,
				operationIsLocatedInWorkspace: false,
			}
			const completeMessage = JSON.stringify(sharedMessageProps)

			if (config.callbacks.shouldAutoApproveTool(this.name)) {
				// Auto-approve flow
				await config.callbacks.removeLastPartialMessageIfExistsWithType("ask", "tool")
				await config.callbacks.say("tool", completeMessage, undefined, undefined, false)
			} else {
				// Manual approval flow
				showNotificationForApproval(
					`di wants to fetch content from ${url}`,
					config.autoApprovalSettings.enableNotifications,
				)
				await config.callbacks.removeLastPartialMessageIfExistsWithType("say", "tool")

				const { didApprove } = await ToolResultUtils.askApprovalAndPushFeedback("tool", completeMessage, config)
				if (!didApprove) {
					return formatResponse.toolDenied()
				}
			}

			// Run PreToolUse hook after approval but before execution
			try {
				const { ToolHookUtils } = await import("../utils/ToolHookUtils")
				await ToolHookUtils.runPreToolUseIfEnabled(config, block)
			} catch (error) {
				const { PreToolUseHookCancellationError } = await import("@core/hooks/PreToolUseHookCancellationError")
				if (error instanceof PreToolUseHookCancellationError) {
					return formatResponse.toolDenied()
				}
				throw error
			}

			// Execute the actual fetch
			const baseUrl = DiracEnv.config().apiBaseUrl
			const authToken = undefined

			if (!authToken) {
				throw new Error(DIRAC_ACCOUNT_AUTH_ERROR_MESSAGE)
			}

			const response = await axios.post(
				`${baseUrl}/api/v1/search/webfetch`,
				{
					Url: url,
					Prompt: prompt,
				},
				{
					headers: {
						Authorization: `Bearer ${authToken}`,
						"Content-Type": "application/json",
						"X-Task-ID": config.ulid || "",
						...(await buildDiracExtraHeaders()),
					},
					timeout: 15000,
					...getAxiosSettings(),
				},
			)

			// Parse response
			// Axios will throw on non-200 status, so no need to check fetchStatus
			const result = response.data.data.result

			return formatResponse.toolResult(result)
		} catch (error) {
			return `Error fetching web content: ${(error as Error).message}`
		}
	}
}
