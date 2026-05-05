import type { ToolUse } from "@core/assistant-message"
import { formatResponse } from "@core/prompts/responses"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import { showNotificationForApproval } from "../../utils"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { ToolResultUtils } from "../utils/ToolResultUtils"
import type { ConstraintViolation } from "@shared/tool-response"

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

	private checkWriteExecuteRisk(command: string, writtenFiles: Set<string>): ConstraintViolation[] {
		if (writtenFiles.size === 0) return []
		const violations: ConstraintViolation[] = []
		const scriptPattern = /\b(\S+\.(sh|py|rb|js|ts|pl))\b/g
		let match: RegExpExecArray | null
		while ((match = scriptPattern.exec(command)) !== null) {
			const candidate = match[1]
			for (const written of writtenFiles) {
				if (written.endsWith(candidate) || candidate.endsWith(written)) {
					violations.push({ path: "$.command", constraint: "executing AI-written file", detected_pattern: candidate, alternatives: ["read the file first, then execute"] })
				}
			}
		}
		return violations
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const command = (block.params.command as string) || ""

		// Security: check if command references a file written this turn
		const securityViolations = this.checkWriteExecuteRisk(command, config.taskState.filesEditedInCurrentTurn)

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


		// Execute via command daemon
		await config.controller.ensureCommandClient(config.cwd)
		const client = config.controller.getCommandClient()

		if (!client || client.fallback) {
			config.taskState.consecutiveMistakeCount++
			return '<tool_error severity="unrecoverable">Command daemon is not available. Ensure dirac-cmd binary exists in the dist directory.</tool_error>'
		}

		const result = await client.execute(command)

		config.taskState.consecutiveMistakeCount = result.exit_code === 0 ? 0 : config.taskState.consecutiveMistakeCount + 1

		// Build plain-text output so the envelope wraps it in pipe format
		let output = `exit:${result.exit_code}`
		if (result.stdout) output += `
${result.stdout}`
		if (result.stderr) output += `
[stderr]
${result.stderr}`
		if (result.meta.truncated) output += `\n[truncated]`
		if (result.meta.timed_out) output += `\n[timed out]`
		if (result.meta.blocked) output += `
[blocked: ${result.meta.blocked}]`
		if (result.meta.detected_patterns?.length) output += `
[detected_patterns: ${result.meta.detected_patterns.join(", ")}]`
		if (securityViolations.length > 0) output += `
[security: ${securityViolations.map(v => v.constraint).join(", ")}]`

		return formatResponse.toolResult(output)
	}
}
