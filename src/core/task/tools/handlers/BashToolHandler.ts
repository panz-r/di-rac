import { spawn } from "node:child_process"
import path from "node:path"
import * as shellQuote from "shell-quote"
import type { ToolUse } from "@core/assistant-message"
import { formatResponse } from "@core/prompts/responses"
import { normalizePath } from "@/utils/path-utils"
import { createToolError } from "@shared/tool-response"
import { telemetryService } from "@/services/telemetry"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import { showNotificationForApproval } from "../../utils"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { ToolResultUtils } from "../utils/ToolResultUtils"

const MAX_BASH_OUTPUT_SIZE = 400 * 1024 // 400KB
const BASH_TIMEOUT_MS = 30000 // 30 seconds
const MAX_PATH_LENGTH = 255 // Linux/macOS single path component limit

const ALLOWED_BINARIES = new Set([
	"git",
	"grep",
	"find",
	"cat",
	"head",
	"tail",
	"jq",
	"wc",
	"sort",
	"uniq",
	"curl",
	"sed",
	"awk",
	"python3",
	"node",
	"ls", // Adding ls as it's essential
])

export class BashToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.BASH_RESTRICTED

	constructor(private validator: ToolValidator) {}

	getDescription(block: ToolUse): string {
		const command = (block.params.command as string) || ""
		return `[${this.name} '${command.slice(0, 50)}${command.length > 50 ? "..." : ""}']`
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

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const rawCommand = (block.params.command as string) || ""

		if (!rawCommand) {
			config.taskState.consecutiveMistakeCount++
			return await config.callbacks.sayAndCreateMissingParamError(this.name, "command")
		}

		// 1. Parse command string into program + args
		const parsed = shellQuote.parse(rawCommand)
		// We only support simple commands, not operators like |, &&, etc. for now if we want strict allowlist.
		// Wait, the plan says "composition like 'grep | head' is encouraged".
		// If we want to support pipes, we can't easily check the allowlist for every command in the pipe.
		// For the first version, let's allow it but check all commands in the pipe if possible.
		
		const entries = parsed.filter((e): e is string => typeof e === "string")
		const binaries = parsed.filter((e, i) => typeof e === "string" && (i === 0 || parsed[i-1] === "|" || parsed[i-1] === "&&" || parsed[i-1] === "||"))
		
		for (const bin of binaries) {
			if (typeof bin === "string" && !ALLOWED_BINARIES.has(bin)) {
				const resultObj = {
					ok: false,
					error: "BINARY_NOT_ALLOWED",
					message: `Command '${bin}' is not allowed. Allowed binaries: ${Array.from(ALLOWED_BINARIES).join(", ")}`
				}
				return formatResponse.toolResult(JSON.stringify(resultObj, null, 2))
			}
		}

		// 1b. Validate: reject path-like arguments exceeding OS filename length limit
		for (const entry of entries) {
			if (
				(entry.startsWith("/") || entry.startsWith("./") || entry.startsWith("../") || entry.includes("/")) &&
				Buffer.byteLength(entry) > MAX_PATH_LENGTH
			) {
				const preview = entry.slice(0, 80)
				const resultObj = {
					ok: false,
					error: "PATH_TOO_LONG",
					message: `Path argument exceeds maximum allowed length (${MAX_PATH_LENGTH} bytes). Saw: ${preview}${entry.length > 80 ? "..." : ""} (total ${Buffer.byteLength(entry)} bytes). If you meant to pass file contents, use a pipe or write to a file first.`
				}
				return formatResponse.toolResult(JSON.stringify(resultObj, null, 2))
			}
		}

		// 2. Path Normalization (if enabled)
		const rewritePaths = config.services.stateManager.getGlobalSettingsKey("rewritePaths")
		let finalCommand = rawCommand
		
		if (rewritePaths) {
			finalCommand = parsed.map(entry => {
				if (typeof entry === "string") {
					// heuristic: if it looks like a path, normalize it
					if (entry.startsWith("/") || entry.startsWith("./") || entry.startsWith("../") || entry.includes("/")) {
						try {
							const normalized = normalizePath(entry, config.cwd)
							return shellQuote.quote([normalized])
						} catch {
							return shellQuote.quote([entry])
						}
					}
					return shellQuote.quote([entry])
				}
				if ("op" in entry) {
					return entry.op
				}
				return ""
			}).join(" ")
		}

		// 3. Validation: Path Traversal
		if (finalCommand.includes("../")) {
			const resultObj = {
				ok: false,
				error: "PATH_ESCAPE",
				message: "Path traversal using '../' is not allowed for security reasons."
			}
			return formatResponse.toolResult(JSON.stringify(resultObj, null, 2))
		}

		// 5. Approval: check bashAutoApproveAll before requiring manual approval
		const bashAutoApproveAll = config.services.stateManager.getGlobalSettingsKey("bashAutoApproveAll")

		if (!bashAutoApproveAll) {
			showNotificationForApproval(
				`Dirac wants to execute: ${finalCommand}`,
				config.autoApprovalSettings.enableNotifications
			)

			const { didApprove } = await ToolResultUtils.askApprovalAndPushFeedback(
				"command",
				finalCommand,
				config,
				false,
				{ commands: [{ command: finalCommand, status: "pending" }] }
			)

			if (!didApprove) {
				return formatResponse.toolResult("Command denied by user.")
			}
		}

		// 6. Execution
		const apiConfig = config.services.stateManager.getApiConfiguration()
		const currentMode = config.services.stateManager.getGlobalSettingsKey("mode")
		const provider = (currentMode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string

		telemetryService.captureToolUsage(
			config.ulid,
			this.name,
			config.api.getModel().id,
			provider,
			false, // didAutoApprove is false
			true,
			undefined,
			block.isNativeToolCall
		)

		return new Promise((resolve) => {
			const child = spawn("rbash", ["--norc", "--noprofile", "-c", finalCommand], {
				cwd: config.cwd,
				env: { ...process.env, PATH: "/usr/bin:/bin" }, // Restrict PATH as per plan
			})

			let stdout = ""
			let stderr = ""
			let killed = false

			const timeout = setTimeout(() => {
				killed = true
				child.kill()
			}, BASH_TIMEOUT_MS)

			child.stdout.on("data", (data) => {
				if (stdout.length < MAX_BASH_OUTPUT_SIZE) {
					stdout += data.toString()
				}
			})

			child.stderr.on("data", (data) => {
				if (stderr.length < MAX_BASH_OUTPUT_SIZE) {
					stderr += data.toString()
				}
			})

			child.on("close", (code) => {
				clearTimeout(timeout)
				
				const isTruncated = stdout.length >= MAX_BASH_OUTPUT_SIZE || stderr.length >= MAX_BASH_OUTPUT_SIZE

				if (stdout.length >= MAX_BASH_OUTPUT_SIZE) {
					stdout = stdout.slice(0, MAX_BASH_OUTPUT_SIZE) + "\n--- [OUTPUT TRUNCATED] ---"
				}
				if (stderr.length >= MAX_BASH_OUTPUT_SIZE) {
					stderr = stderr.slice(0, MAX_BASH_OUTPUT_SIZE) + "\n--- [STDERR TRUNCATED] ---"
				}

				const resultObj = {
					ok: !killed && code === 0,
					exitCode: code,
					stdout: stdout || undefined,
					stderr: stderr || undefined,
					truncated: isTruncated || undefined,
					error: killed ? "TIMEOUT" : undefined
				}
				
				config.taskState.consecutiveMistakeCount = 0
				resolve(formatResponse.toolResult(JSON.stringify(resultObj, null, 2)))
			})

			child.on("error", (err) => {
				clearTimeout(timeout)
				const resultObj = {
					ok: false,
					error: "SPAWN_ERROR",
					message: `Failed to start process: ${err.message}`
				}
				resolve(formatResponse.toolResult(JSON.stringify(resultObj, null, 2)))
			})
		})
	}
}
