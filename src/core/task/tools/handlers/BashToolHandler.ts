import type { ToolUse } from "@core/assistant-message"
import { formatResponse } from "@core/prompts/responses"
import { telemetryService } from "@/services/telemetry"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import { showNotificationForApproval } from "../../utils"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { ToolResultUtils } from "../utils/ToolResultUtils"

export class BashToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.BASH

	constructor(private validator: ToolValidator) {}

	getDescription(block: ToolUse): string {
		const command = (block.params.command as string) || ""
		return `bash '${command.slice(0, 60)}${command.length > 60 ? "..." : ""}'`
	}

	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		const config = uiHelpers.getConfig()
		if (config.isSubagentExecution) return

		const bashAutoApproveAll = config.services.stateManager.getGlobalSettingsKey("bashAutoApproveAll")
		if (bashAutoApproveAll) return

		const command = (block.params.command as string) || ""
		await uiHelpers.removeLastPartialMessageIfExistsWithType("say", "tool")
		await uiHelpers.ask("tool", command, block.partial, {
			commands: [{ command, status: "pending" }]
		}).catch(() => {})
	}

	private checkWriteExecuteRisk(command: string, writtenFiles: Set<string>): string | null {
		if (writtenFiles.size === 0) return null
		const scriptPattern = /\b(\S+\.(sh|py|rb|js|ts|pl))\b/g
		let match: RegExpExecArray | null
		while ((match = scriptPattern.exec(command)) !== null) {
			const candidate = match[1]
			for (const written of writtenFiles) {
				if (written.endsWith(candidate) || candidate.endsWith(written)) {
					return `[SECURITY] Executing AI-written file '${candidate}'. Consider reviewing with read first.`
				}
			}
		}
		return null
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const command = (block.params.command as string) || ""

		// Security: check if command references a file written this turn
		const securityWarning = this.checkWriteExecuteRisk(command, config.taskState.filesEditedInCurrentTurn)

		if (!command) {
			config.taskState.consecutiveMistakeCount++
			return await config.callbacks.sayAndCreateMissingParamError(this.name, "command")
		}

		// Approval
		const bashAutoApproveAll = config.services.stateManager.getGlobalSettingsKey("bashAutoApproveAll")
		const isYolo = config.yoloModeToggled || config.services.stateManager.getGlobalSettingsKey("autoApproveAllToggled")

		if (!bashAutoApproveAll && !isYolo) {
			showNotificationForApproval(
				`di wants to execute: ${command}`,
				config.autoApprovalSettings.enableNotifications
			)

			const { didApprove } = await ToolResultUtils.askApprovalAndPushFeedback(
				"command",
				command,
				config,
				false,
				{ commands: [{ command, status: "pending" }] }
			)

			if (!didApprove) {
				return formatResponse.toolResult("Command denied by user.")
			}
		}

		// Telemetry
		const apiConfig = config.services.stateManager.getApiConfiguration()
		const currentMode = config.services.stateManager.getGlobalSettingsKey("mode")
		const provider = (currentMode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string

		telemetryService.captureToolUsage(
			config.ulid,
			this.name,
			config.api.getModel().id,
			provider,
			false,
			true,
			undefined,
			block.isNativeToolCall
		)

		// Execute via command daemon
		await config.controller.ensureCommandClient(config.cwd)
		const client = config.controller.getCommandClient()

		if (!client || client.fallback) {
			const resultObj = {
				ok: false,
				error: "DAEMON_UNAVAILABLE",
				message: "Command daemon is not available. Ensure dirac-cmd binary exists in the dist directory."
			}
			config.taskState.consecutiveMistakeCount++
			return formatResponse.toolResult(JSON.stringify(resultObj, null, 2))
		}

		const result = await client.execute(command)

		const resultObj = {
			ok: result.exit_code === 0,
			exitCode: result.exit_code,
			stdout: result.stdout || undefined,
			stderr: result.stderr || undefined,
			truncated: result.meta.truncated || undefined,
			timedOut: result.meta.timed_out || undefined,
			cwd: result.meta.cwd,
		}

		config.taskState.consecutiveMistakeCount = result.exit_code === 0 ? 0 : config.taskState.consecutiveMistakeCount + 1

		let output = JSON.stringify(resultObj, null, 2)
		if (securityWarning) output = securityWarning + '\n' + output
		return formatResponse.toolResult(output)
	}
}
