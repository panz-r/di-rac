/**
 * Plain-text task runner for non-TTY environments (piped output, file redirection)
 * Optimized for CI/CD and piping - only outputs the final completion result to stdout.
 *
 * Design goals:
 * - stdout: Only the final completion result text (no prefix) - perfect for piping
 * - stderr: Errors and verbose output (won't break pipes)
 * - Enables workflows like: git diff | dirac 'explain' | dirac 'summarize'
 */

/* eslint-disable no-console */
// Console output is intentional here for plain text mode

import type { DiracMessage, ExtensionState } from "@shared/ExtensionMessage"
import { StringRequest } from "../proto/dirac"
import type { Controller } from "@/core/controller"
import { getRequestRegistry } from "@/core/controller/grpc-handler"
import { subscribeToState } from "@/core/controller/state/subscribeToState"
import { showTaskWithId } from "@/core/controller/task/showTaskWithId"
import { emitTaskStartedMessage } from "./task-start-output"
import { getApiMetrics } from "@shared/getApiMetrics"

export interface PlainTextTaskOptions {
	controller: Controller
	/** Prompt for new task or message to send to resumed task */
	prompt?: string
	imageDataUrls?: string[]
	verbose?: boolean
	jsonOutput?: boolean
	/** Timeout in seconds (only applied when explicitly provided) */
	timeoutSeconds?: number
	/** Task ID to resume an existing task */
	taskId?: string
	yolo?: boolean
}

/**
 * Run a task with plain text output (no Ink, no ANSI codes)
 * Returns true if task completed successfully, false if error
 *
 * Output behavior:
 * - Non-JSON mode: Only writes final completion_result text to stdout
 * - JSON mode: Streams JSON lines to stdout as messages arrive (unchanged)
 * - Verbose mode: Progress info goes to stderr
 * - Errors: Always go to stderr
 */
export async function runPlainTextTask(options: PlainTextTaskOptions): Promise<boolean> {
	const { controller, prompt, imageDataUrls, verbose, jsonOutput, yolo } = options

	let completionResolve: (reason?: any) => void
	let completionReject: (reason?: any) => void
	const completionPromise = new Promise<string>((res, rej) => {
		completionResolve = res
		completionReject = rej
	})

	let hasError = false
	let hasEmittedTaskStarted = false
	// Track which messages have been processed (by timestamp)
	const processedMessages = new Map<number, string>()
	const lastProcessedPartialMessages = new Map<number, DiracMessage>()

	const isViewTaskOnly = Boolean(options.taskId) && !prompt

	// When resuming a task, we need to ignore completion_result messages that existed
	// before we sent our new prompt. This timestamp marks the cutoff - only completion
	// results AFTER this time should trigger task completion.
	const completionCutoffTs = Date.now()

	const printPendingPartials = () => {
		for (const partialMsg of Array.from(lastProcessedPartialMessages.values()).sort(
			(a, b) => (a.ts || 0) - (b.ts || 0),
		)) {
			handleMessageForPipeMode(partialMsg, true, yolo || false)
		}
		lastProcessedPartialMessages.clear()
	}


	const emitTaskStarted = () => {
		if (hasEmittedTaskStarted) {
			return
		}

		const taskId = controller.task?.taskId
		if (!taskId) {
			return
		}

		emitTaskStartedMessage(taskId, Boolean(jsonOutput))
		hasEmittedTaskStarted = true
	}

	// Helper to process a message and track completion state
	const processMessage = (message: DiracMessage) => {
		const text = message.text || ""
		const ts = message.ts || 0

		if (message.partial) {
			if (!jsonOutput && verbose) {
				lastProcessedPartialMessages.set(ts, message)
			}
			return
		}

		// Message is complete
		lastProcessedPartialMessages.delete(ts)

		if (processedMessages.has(ts) && processedMessages.get(ts) === text) {
			return
		}

		// JSON mode: stream all messages to stdout (existing behavior)
		if (jsonOutput) {
			process.stdout.write(JSON.stringify(message) + "\n")
		} else {
			handleMessageForPipeMode(message, verbose || false, yolo || false, processedMessages.has(ts))
		}

		processedMessages.set(ts, text)

		// Auto-approve if yolo mode is on and it's an approval request
		if (
			yolo &&
			message.type === "ask" &&
			(message.ask === "tool" ||
				message.ask === "command" ||
				message.ask === "browser_action_launch" ||
				message.ask === "plan_mode_respond" ||
				message.ask === "act_mode_respond")
		) {
			controller.task?.handleWebviewAskResponse("yesButtonClicked")
		}

		// Check for completion (only on non-partial messages)
		// When resuming a task, only consider completion_result messages that appeared
		// AFTER we sent our resume message (ts > completionCutoffTs)
		if (message.say === "completion_result" || message.ask === "completion_result") {
			if (isViewTaskOnly || ts > completionCutoffTs) {
				completionResolve()
			}
		} else if (message.say === "error" || message.ask === "api_req_failed") {
			completionReject(message.text ?? "message.say error || message.ask api_req_failed")
		}
	}

	const requestId = "dirac-plain-text-task"
	subscribeToState(
		controller,
		{},
		async ({ stateJson }) => {
			try {
				const state = JSON.parse(stateJson) as ExtensionState
				for (const message of state.diracMessages ?? []) {
					processMessage(message)
				}
			} catch (error) {
				if (jsonOutput) {
					process.stdout.write(
						JSON.stringify({ type: "error", message: error instanceof Error ? error.message : String(error) }) +
							"\n",
					)
				} else {
					process.stderr.write(`Error: ${error instanceof Error ? error.message : String(error)}\n`)
				}
				completionReject(error)
			}
		},
		requestId,
	)

	try {
		// Either resume an existing task or start a new one
		if (options.taskId) {
			// Load the existing task
			await showTaskWithId(controller, StringRequest.create({ value: options.taskId }))
			emitTaskStarted()

			// If a prompt was provided, send it as a message to the resumed task
			if (prompt && controller.task) {
				// Wait a moment for the task to fully load
				await new Promise((resolve) => setTimeout(resolve, 100))

				// Send the prompt as a response to any pending ask, or as a new message
				await controller.task.handleWebviewAskResponse("messageResponse", prompt)
			}
		} else if (prompt) {
			// Start a new task with the prompt
			await controller.initTask(prompt, imageDataUrls)
			emitTaskStarted()
		} else {
			throw new Error("Either taskId or prompt must be provided")
		}

		// Wait for task completion, with optional timeout only when explicitly configured
		if (options.timeoutSeconds) {
			const timeoutMs = options.timeoutSeconds * 1000
			const timeoutPromise = new Promise((_, reject) =>
				setTimeout(() => reject(new Error("Timeout")), timeoutMs),
			)
			await Promise.race([completionPromise, timeoutPromise])
		} else {
			await completionPromise
		}
	} catch (error) {
		printPendingPartials()
		const errMsg = error instanceof Error ? error.message : String(error)
		if (jsonOutput) {
			process.stdout.write(JSON.stringify({ type: "error", message: errMsg }) + "\n")
		} else {
			process.stderr.write(`Error: ${errMsg}\n`)
		}
		hasError = true
	} finally {
		getRequestRegistry().cancelRequest(requestId)
	}

	// non json mode outputs only the final complete message
	// (it should be the completion_result message)
	if (!jsonOutput) {
		const msg = Array.from(processedMessages.entries())
			.sort(([aTs], [bTs]) => aTs - bTs)
			.map(([_, msg]) => msg)
			.at(-1)
		if (msg) {
			process.stdout.write(msg + "\n")
		}
	}

	// Print final summary if verbose or yolo
	if (!jsonOutput && (verbose || yolo)) {
		const messages = controller.task?.messageStateHandler.getDiracMessages() || []
		const metrics = getApiMetrics(messages)
		if (metrics.totalTokensIn > 0) {
			process.stderr.write(`\n${"-".repeat(40)}\n`)
			process.stderr.write(`Task Summary:\n`)
			process.stderr.write(
				`Tokens: ${metrics.totalTokensIn.toLocaleString()} in, ${metrics.totalTokensOut.toLocaleString()} out${metrics.totalReasoningTokens ? ` (+${metrics.totalReasoningTokens.toLocaleString()} thinking)` : ""}\n`
			)
			if (metrics.totalCacheReads || metrics.totalCacheWrites) {
				process.stderr.write(
					`Cache: ${(metrics.totalCacheReads || 0).toLocaleString()} read, ${(metrics.totalCacheWrites || 0).toLocaleString()} write\n`,
				)
			}
			process.stderr.write(`${"-".repeat(40)}\n`)
		}
	}

	return !hasError
}

/**
 * Handle a message in pipe-optimized mode (non-JSON)
 * - Assistant response text (say: "text") is passed to the callback for buffering
 * - Errors go to stderr
 * - Verbose output goes to stderr
 * - Nothing else goes to stdout (stdout is reserved for final result only)
 */
function handleMessageForPipeMode(
	message: DiracMessage,
	verbose: boolean,
	yolo: boolean,
	isUpdate?: boolean,
): void {
	const fullText = message.text ?? ""
	const reasoning = message.reasoning ?? ""
	const isPartial = message.partial ?? false
	const statusPrefix = verbose ? (isPartial ? "[partial] " : "[complete] ") : ""

	if (message.type === "say") {
		if (message.say === "error") {
			// Errors always go to stderr
			process.stderr.write(`${statusPrefix}Error: ${fullText}\n`)
		} else if (verbose) {
			// Verbose output goes to stderr so it doesn't interfere with piped stdout
			if (message.say === "task") {
				process.stderr.write(`${statusPrefix}${fullText}\n`)
			} else if (message.say === "text" && fullText) {
				process.stderr.write(`${statusPrefix}${fullText}\n`)
			} else if (message.say === "api_req_started" || message.say === "api_req_finished") {
				const label = message.say === "api_req_started" ? "API request started" : "API request finished"
				try {
					const info = JSON.parse(fullText || "{}")
					const hasMetrics = info.tokensIn !== undefined
					if (hasMetrics || !isUpdate) {
						const tokensStr =
							info.tokensIn !== undefined
								? `Tokens: ${info.tokensIn.toLocaleString()} in, ${info.tokensOut.toLocaleString()} out${info.reasoningTokens ? ` (+${info.reasoningTokens.toLocaleString()} thinking)` : ""}`
								: ""
						const cacheStr =
							info.cacheReads !== undefined || info.cacheWrites !== undefined
								? ` (Cache: ${(info.cacheReads || 0).toLocaleString()} read, ${(info.cacheWrites || 0).toLocaleString()} write)`
								: ""
						const contextStr =
							info.contextWindow !== undefined
								? ` | Context: ${info.contextUsagePercentage}% of ${(info.contextWindow / 1000).toFixed(0)}K`
								: ""

						const metricsStr = hasMetrics ? ` [${tokensStr}${cacheStr}${contextStr}]` : ""
						process.stderr.write(`${statusPrefix}${label}${metricsStr}\n`)
					}
				} catch {
					process.stderr.write(`${statusPrefix}${label}\n`)
				}
			} else if (message.say === "completion_result" && fullText) {
				process.stderr.write(`${statusPrefix}Completion Result: ${fullText}\n`)
			} else if (message.say === "reasoning" || reasoning) {
				const content = fullText || reasoning
				if (content) {
					process.stderr.write(`${statusPrefix}Reasoning: ${content}\n`)
				}
			} else if (message.say === "tool") {
				process.stderr.write(`${statusPrefix}Tool Call: ${fullText}\n`)
			} else if (message.say === "command") {
				process.stderr.write(`${statusPrefix}Command: ${fullText}\n`)
			} else if (message.say === "command_output") {
				process.stderr.write(`${statusPrefix}Command Output: ${fullText}\n`)
			} else if (fullText) {
				process.stderr.write(`${statusPrefix}${message.say}: ${fullText}\n`)
			} else {
				// Catch-all for other say types in verbose mode
				process.stderr.write(`${statusPrefix}Event: ${message.say}\n`)
			}
		}
	} else if (message.type === "ask") {
		if (message.ask === "api_req_failed") {
			// Errors always go to stderr
			process.stderr.write(`${statusPrefix}Error: API request failed: ${fullText}\n`)
		} else if (
			yolo &&
			(message.ask === "tool" ||
				message.ask === "command" ||
				message.ask === "browser_action_launch" ||
				message.ask === "plan_mode_respond" ||
				message.ask === "act_mode_respond")
		) {
			// In yolo mode, we auto-approve everything that requires it
			if (verbose && statusPrefix === "[complete] ") {
				process.stderr.write(`${statusPrefix}[yolo] Auto-approving ${message.ask}: ${fullText}\n`)
			}
		} else if (message.ask === "tool" || message.ask === "command" || message.ask === "browser_action_launch") {
			// These require approval - warn via stderr
			process.stderr.write(
				`${statusPrefix}Waiting for approval (use --yolo for auto-approve): ${message.ask}: ${fullText}\n`,
			)
		} else if (verbose) {
			// Verbose output goes to stderr
			if (message.ask === "plan_mode_respond" || message.ask === "act_mode_respond") {
				if (fullText) {
					process.stderr.write(`${statusPrefix}Response: ${fullText}\n`)
				}
			} else if (message.ask === "completion_result") {
				process.stderr.write(`${statusPrefix}Task completed\n`)
			} else if (fullText) {
				process.stderr.write(`${statusPrefix}Question: ${fullText}\n`)
			} else {
				// Catch-all for other ask types in verbose mode
				process.stderr.write(`${statusPrefix}Question Type: ${message.ask}\n`)
			}
		}
	}
}
