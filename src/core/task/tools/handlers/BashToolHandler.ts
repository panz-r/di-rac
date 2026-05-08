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

		// Handle --await <id> to retrieve result of a previously-started command
		const awaitMatch = command.match(/^--await\s+(\d+)\s*$/)
		if (awaitMatch) {
			return this.handleAwait(config, awaitMatch[1], securityViolations)
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

		// Parse --timeout N from command (stripped before execution)
		let timeoutSeconds: number | undefined
		let execCommand = command
		const timeoutMatch = command.match(/\s--timeout\s+(\d+)\s*$/)
		if (timeoutMatch) {
			const maxTimeout = config.services.stateManager.getGlobalSettingsKey("bashMaxTimeout") || 120
			timeoutSeconds = Math.min(parseInt(timeoutMatch[1], 10), maxTimeout)
			execCommand = command.slice(0, timeoutMatch.index).trimEnd()
		}

		// Execute via command daemon
		await config.controller.ensureCommandClient(config.cwd)
		const client = config.controller.getCommandClient()

		if (!client || client.fallback) {
			config.taskState.consecutiveMistakeCount++
			return '<tool_error severity="unrecoverable">Command daemon is not available. Ensure di-rvv-cmd binary exists in the dist directory.</tool_error>'
		}

		// Use turn-delay: wait up to maxTurnDelay for result, return partial if still running
		const maxDelay = config.services.stateManager.getGlobalSettingsKey("bashTurnDelayMs") || 10000
		const result = await client.executeWithDelay(execCommand, maxDelay, undefined, timeoutSeconds)

		// Still running — return partial result for LLM
		if ("status" in result && (result as any).status === "running") {
			return this.formatRunningResult(result as import("@/services/command/CommandClient").RunningCommandStatus)
		}

		// Full result — format as before
		return this.formatCompleteResult(config, result as import("@/services/command/CommandClient").CommandResult, securityViolations)
	}

	private async handleAwait(config: TaskConfig, commandId: string, securityViolations: ConstraintViolation[]): Promise<ToolResponse> {
		await config.controller.ensureCommandClient(config.cwd)
		const client = config.controller.getCommandClient()

		if (!client || client.fallback) {
			config.taskState.consecutiveMistakeCount++
			return '<tool_error severity="unrecoverable">Command daemon is not available.</tool_error>'
		}

		try {
			const result = await client.awaitResult(commandId)
			return this.formatCompleteResult(config, result, securityViolations)
		} catch (e) {
			config.taskState.consecutiveMistakeCount++
			const msg = e instanceof Error ? e.message : String(e)
			return formatResponse.toolResult(`exit:error\n[await failed] ${msg}`)
		}
	}

	private formatRunningResult(status: import("@/services/command/CommandClient").RunningCommandStatus): ToolResponse {
		const tail = status.progress.stdout_tail || "(no output yet)"
		const elapsed = Math.round(status.progress.elapsed_ms / 1000)
		return formatResponse.toolResult(
			`exit:running\n` +
			`[Command still running — ${elapsed}s elapsed, ${status.progress.stdout_bytes} bytes output]\n` +
			`Recent output:\n${tail}\n` +
			`To get the final result: bash --await ${status.id}\n` +
			`The command continues running in the background.`,
		)
	}

	private formatCompleteResult(config: TaskConfig, result: import("@/services/command/CommandClient").CommandResult, securityViolations: ConstraintViolation[]): ToolResponse {
		config.taskState.consecutiveMistakeCount = result.exit_code === 0 ? 0 : config.taskState.consecutiveMistakeCount + 1

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
			if (result.meta.hint) output += `\n[hint: ${result.meta.hint}]`

		return formatResponse.toolResult(output)
	}
}
