import { ToolUse } from "@core/assistant-message"
import { formatResponse } from "@core/prompts/responses"
import { ToolResponse } from "@core/task"
import { processFilesIntoText } from "@/integrations/misc/extract-text"
import { DiracAsk, MultiCommandState } from "@/shared/ExtensionMessage"
import { Logger } from "@/shared/services/Logger"
import type { ToolOutputEnvelope } from "@/shared/tool-response"
import type { ToolExecutorCoordinator } from "../ToolExecutorCoordinator"
import { TaskConfig } from "../types/TaskConfig"

/**
 * Utility functions for handling tool results and feedback
 */
export class ToolResultUtils {
	/**
	 * Push tool result to user message content with proper formatting
	 */
	static pushToolResult(
		content: ToolResponse,
		block: ToolUse,
		userMessageContent: any[],
		toolDescription: (block: ToolUse) => string,
		coordinator?: ToolExecutorCoordinator,
		toolUseIdMap?: Map<string, string>,
		metrics?: { cumulativeTokens: number; readCount: number },
	): void {
		if (typeof content === "string") {
			const wrapped = ToolResultUtils.wrapInEnvelope(content, block.name, metrics)
			const resultText = wrapped || content || "(tool did not return anything)"

			// Try to get description from coordinator first, otherwise use the provided function
			const description = coordinator
				? (() => {
						const handler = coordinator.getHandler(block.name)
						return handler ? handler.getDescription(block) : toolDescription(block)
					})()
				: toolDescription(block)

			// Get tool_use_id from map using call_id, or use "dirac" as fallback for backward compatibility
			const toolUseId = toolUseIdMap?.get(block.call_id || "") || "dirac"

			// If we have already added a tool result for this tool use, skip adding another one
			if (
				userMessageContent.some((item) => item.type === "tool_result" && item.tool_use_id === toolUseId && item.content)
			) {
				Logger.warn(`ToolResultUtils: Tool result for tool_use_id ${toolUseId} already exists. Skipping duplicate.`)
				return
			}

			// Create ToolResultBlockParam with description and result
			userMessageContent.push(
				ToolResultUtils.createToolResultBlock(`${description} Result:\n${resultText}`, toolUseId, block.call_id),
			)
		} else {
			// For complex content (arrays with text/image blocks), pass it through directly
			// The content array should already be properly formatted with type, text, source, etc.
			const toolUseId = toolUseIdMap?.get(block.call_id || "") || "dirac"

			// If using backward-compatible "dirac" ID and content is an array, spread it directly
			// instead of wrapping it (which would cause JSON.stringify in createToolResultBlock)
			if ((toolUseId === "dirac" || !toolUseId) && Array.isArray(content)) {
				userMessageContent.push(...content)
			} else {
				userMessageContent.push(ToolResultUtils.createToolResultBlock(content, toolUseId, block.call_id))
			}
		}
	}

	private static createToolResultBlock(content: ToolResponse, id?: string, call_id?: string) {
		// If id is "dirac", we treat it as a plain text result for backward compatibility
		// as we cannot find any existing tool call that matches this id.
		if (id === "dirac" || !id) {
			return {
				type: "text",
				text: typeof content === "string" ? content : JSON.stringify(content, null, 2),
			}
		}

		// For tool_result blocks, content can be either a string or an array of content blocks
		// When it's a string, we need to wrap it in the proper format
		// When it's an array, it should already be properly formatted (e.g., with text and image blocks)
		return {
			type: "tool_result",
			tool_use_id: id,
			call_id: call_id,
			content: typeof content === "string" ? content : content,
		}
	}

	/**
	 * Wrap a string tool result in a JSON envelope for structured LLM consumption.
	 * Passes through content that is already a JSON envelope (e.g. bash output).
	 *
	 * Envelope shape: { status, data, hint, meta, error? }
	 *   status: "ok" | "error" | "truncated" | "empty"
	 *   hint: top-level, present on ALL response types
	 *   meta: tokens (estimate), truncated, lines, tool-specific extras
	 */
	private static wrapInEnvelope(content: string, toolName: string, metrics?: { cumulativeTokens: number; readCount: number }): string | null {
		// Skip wrapping if content looks like it\'s already structured JSON
		const trimmed = content.trimStart()
		if (trimmed.startsWith("{")) {
			try {
				const parsed = JSON.parse(trimmed)
				if (parsed.status !== undefined || parsed.ok !== undefined) return null
			} catch {
				// Not valid JSON, fall through to wrapping
			}
		}

		// Detect cache hit prefix from ToolExecutorCoordinator
		let isCached = false
		let workingContent = content
		if (content.startsWith("[Cache Hit]")) {
			isCached = true
			workingContent = content.slice("[Cache Hit]".length)
		}

		const lines = workingContent.split("\n").length
		const tokens = Math.ceil(workingContent.length / 4)
		const isTruncated = content.includes("[truncated]") || content.includes("... [Content reduced")
		const isEmpty = !trimmed || trimmed === "(tool did not return anything)"
			|| trimmed === "No definitions found."
			|| trimmed === "No matches found."
			|| trimmed === "No results found."
			|| /^0 (files|matches|results|symbols)/.test(trimmed)

		// Detect error content (<tool_error severity="...">...</tool_error>)
		if (trimmed.startsWith("<tool_error")) {
			return ToolResultUtils.buildErrorEnvelope(content, toolName, lines, tokens)
		}

		const metricsMeta = metrics ? { cumulative_tokens: metrics.cumulativeTokens, read_count: metrics.readCount } : {}
		const toolTypeMap: Record<string, string> = { read: "file_content", search: "search_results", bash: "command_output", symbols: "symbol_list", repo: "directory_listing" }
		const outputType = toolTypeMap[toolName]

		// Truncated response
		if (isTruncated) {
			const envelope: ToolOutputEnvelope = {
				status: "truncated",
				data: content,
				hint: "Output truncated. Use --range or --detail for targeted reads.",
				meta: { tool: toolName, lines, tokens, truncated: true },
			}
			return JSON.stringify(envelope, null, 2)
		}

		// Empty result (not an error \u2014 the tool ran fine, found nothing)
		if (isEmpty) {
			const hint = ToolResultUtils.getEmptyHint(toolName)
			const envelope: ToolOutputEnvelope = {
				status: "empty",
				data: content || "",
				hint,
				meta: { tool: toolName, lines, tokens, truncated: false },
			}
			return JSON.stringify(envelope, null, 2)
		}

		// Normal success
		const envelope: ToolOutputEnvelope = {
			status: "ok",
			data: workingContent,
			hint: null,
			meta: { tool: toolName, lines, tokens, truncated: false, cached: isCached || undefined, ...metricsMeta },
			...(outputType ? { type: outputType as ToolOutputEnvelope["type"] } : {}),
		}
		return JSON.stringify(envelope, null, 2)
	}

		private static buildErrorEnvelope(content: string, toolName: string, lines: number, tokens: number): string {
		// Parse <tool_error severity="recoverable">message\nSuggested next steps:\n...</tool_error>
		const severityMatch = content.match(/severity="([^"]+)"/)
		const bodyMatch = content.match(/severity="[^"]+">\n?([\s\S]*?)<\/tool_error>/)
		const body = bodyMatch?.[1]?.trim() || content.replace(/<[^>]+>/g, "").trim()

		// Split body into message and suggestion parts
		const parts = body.split(/Suggested next steps:/)
		const message = parts[0]?.trim() || "Tool execution failed."
		const suggestion = parts[1]?.trim() || undefined

		// Extract a short error code from the message
		let code = "TOOL_ERROR"
		if (message.includes("not found") || message.includes("could not be found")) code = "ENOENT"
		else if (message.includes("permission") || message.includes("blocked")) code = "EPERM"
		else if (message.includes("locked")) code = "ELOCK"
		else if (message.includes("anchor")) code = "ANCHOR_MISS"
		else if (message.includes("argument") || message.includes("parameter")) code = "EINVAL"

		const envelope: ToolOutputEnvelope = {
			status: "error",
			data: null,
			hint: suggestion || null,
			meta: { tool: toolName, lines, tokens, truncated: false },
			error: {
				code,
				message: message.slice(0, 500),
			},
		}
		return JSON.stringify(envelope, null, 2)
	}

	private static getEmptyHint(toolName: string): string {
		switch (toolName) {
			case "search":
				return "No matches. Try broader pattern, different path, or --context for surrounding lines."
			case "symbols":
				return "No symbol matches. Try different pattern, --kind function, or use search for text patterns."
			case "repo":
				return "No results. Try different path or --detail files."
			case "read":
				return "No definitions found. File may be empty or unsupported type."
			default:
				return "No results found. Try different parameters."
		}
	}

	/**
	 * Push additional tool feedback from user to message content
	 */
	static pushAdditionalToolFeedback(
		userMessageContent: any[],
		feedback?: string,
		images?: string[],
		fileContentString?: string,
	): void {
		// Check if we have any meaningful content to add
		const hasMeaningfulFeedback = feedback && feedback.trim() !== ""
		const hasImages = images && images.length > 0
		const hasMeaningfulFileContent = fileContentString && fileContentString.trim() !== ""

		// Only proceed if we have at least one meaningful piece of content
		if (!hasMeaningfulFeedback && !hasImages && !hasMeaningfulFileContent) {
			return
		}

		// Build the feedback text only if we have meaningful feedback
		const feedbackText = hasMeaningfulFeedback
			? `The user provided the following feedback:\n<feedback>\n${feedback}\n</feedback>`
			: "The user provided additional content:"

		const content = formatResponse.toolResult(feedbackText, images, hasMeaningfulFileContent ? fileContentString : undefined)
		if (typeof content === "string") {
			userMessageContent.push({
				type: "text",
				text: content,
			})
		} else {
			userMessageContent.push(...content)
		}
	}

	/**
	 * Handles tool approval flow and processes any user feedback
	 */
	static async askApprovalAndPushFeedback(
		type: DiracAsk,
		completeMessage: string | undefined,
		config: TaskConfig,
		partial: boolean = false,
		multiCommandState?: MultiCommandState,
	) {
		if (config.isSubagentExecution) {
			return { didApprove: true, askTs: undefined as number | undefined }
		}

		const result = await config.callbacks.ask(type, completeMessage, partial, multiCommandState)
		const { response, text, images, files } = result

		if (text || (images && images.length > 0) || (files && files.length > 0)) {
			let fileContentString = ""
			if (files && files.length > 0) {
				fileContentString = await processFilesIntoText(files)
			}

			ToolResultUtils.pushAdditionalToolFeedback(config.taskState.userMessageContent, text, images, fileContentString)
			if (!partial) {
				await config.callbacks.say("user_feedback", text, images, files)
			}
		}

		if (partial) {
			return { didApprove: false, ...result }
		}

		if (response !== "yesButtonClicked") {
			// User pressed reject button or responded with a message, which we treat as a rejection
			config.taskState.didRejectTool = true // Prevent further tool uses in this message
			return { didApprove: false, ...result }
		}
		// User hit the approve button, and may have provided feedback
		return { didApprove: true, ...result }
	}
}
