import { Anthropic } from "@anthropic-ai/sdk"
import OpenAI from "openai"
import { DiracAssistantThinkingBlock, DiracStorageMessage } from "@/shared/messages/content"

/**
 * DeepSeek Reasoner message format with reasoning_content support.
 */
export type DeepSeekReasonerMessage = OpenAI.Chat.ChatCompletionMessageParam & {
	reasoning_content?: string
}

/**
 * Adds reasoning_content to OpenAI messages for DeepSeek Reasoner.
 * DeepSeek API requires reasoning_content to be passed back for ALL assistant messages
 * that have it. Failure to do so results in a 400 error:
 * "The reasoning_content in the thinking mode must be passed back to the API."
 */
export function addReasoningContent(
	openAiMessages: OpenAI.Chat.ChatCompletionMessageParam[],
	originalMessages: DiracStorageMessage[],
): DeepSeekReasonerMessage[] {
	// Extract thinking content from original messages, keyed by assistant index
	const thinkingByIndex = new Map<number, string>()
	let assistantIdx = 0
	for (const msg of originalMessages) {
		if (msg.role === "assistant") {
			if (Array.isArray(msg.content)) {
				const thinking = msg.content
					.filter((p): p is DiracAssistantThinkingBlock => p.type === "thinking")
					.map((p) => p.thinking)
					.join("\n")
				if (thinking) {
					thinkingByIndex.set(assistantIdx, thinking)
				}
			}
			assistantIdx++
		}
	}
	// Add reasoning_content to ALL assistant messages that have thinking content
	let aiIdx = 0
	return openAiMessages.map((msg, i): DeepSeekReasonerMessage => {
		if (msg.role === "assistant") {
			const thinking = thinkingByIndex.get(aiIdx++)
			if (thinking) {
				return { ...msg, reasoning_content: thinking }
			}
		}
		return msg
	})
}

/**
 * Converts Anthropic messages to OpenAI format and merges consecutive messages with the same role.
 * This is required for DeepSeek Reasoner which does not support successive messages with the same role.
 * DeepSeek highly recommends using 'user' role instead of 'system' role for optimal performance.
 *
 * @param messages Array of Anthropic messages
 * @returns Array of OpenAI messages where consecutive messages with the same role are merged together
 */
export function convertToR1Format(messages: Anthropic.Messages.MessageParam[]): OpenAI.Chat.ChatCompletionMessageParam[] {
	return messages.reduce<OpenAI.Chat.ChatCompletionMessageParam[]>((merged, message) => {
		const lastMessage = merged[merged.length - 1]
		let messageContent: string | (OpenAI.Chat.ChatCompletionContentPartText | OpenAI.Chat.ChatCompletionContentPartImage)[] =
			""
		let hasImages = false

		if (Array.isArray(message.content)) {
			const textParts: string[] = []
			const imageParts: OpenAI.Chat.ChatCompletionContentPartImage[] = []

			message.content.forEach((part) => {
				if (part.type === "text") {
					textParts.push(part.text || "")
				}
				if (part.type === "image") {
					hasImages = true
					imageParts.push({
						type: "image_url",
						image_url: { url: part.source.type === "base64" ? `data:${part.source.media_type};base64,${part.source.data}` : (part.source as any).url },
					})
				}
			})

			if (hasImages) {
				const parts: (OpenAI.Chat.ChatCompletionContentPartText | OpenAI.Chat.ChatCompletionContentPartImage)[] = []
				if (textParts.length > 0) {
					parts.push({ type: "text", text: textParts.join("\n") })
				}
				parts.push(...imageParts)
				messageContent = parts
			} else {
				messageContent = textParts.join("\n")
			}
		} else {
			messageContent = message.content
		}

		// If the last message has the same role, merge the content
		if (lastMessage?.role === message.role) {
			if (typeof lastMessage.content === "string" && typeof messageContent === "string") {
				lastMessage.content += `\n${messageContent}`
			} else {
				const lastContent = Array.isArray(lastMessage.content)
					? lastMessage.content
					: [{ type: "text" as const, text: lastMessage.content || "" }]

				const newContent = Array.isArray(messageContent)
					? messageContent
					: [{ type: "text" as const, text: messageContent }]

				if (message.role === "assistant") {
					const mergedContent = [
						...lastContent,
						...newContent,
					] as OpenAI.Chat.ChatCompletionAssistantMessageParam["content"]
					lastMessage.content = mergedContent
				} else {
					const mergedContent = [...lastContent, ...newContent] as OpenAI.Chat.ChatCompletionUserMessageParam["content"]
					lastMessage.content = mergedContent
				}
			}
		} else {
			// Adds new message with the correct type based on role
			if (message.role === "assistant") {
				const newMessage: OpenAI.Chat.ChatCompletionAssistantMessageParam = {
					role: "assistant",
					content: messageContent as OpenAI.Chat.ChatCompletionAssistantMessageParam["content"],
				}
				merged.push(newMessage)
			} else {
				const newMessage: OpenAI.Chat.ChatCompletionUserMessageParam = {
					role: "user",
					content: messageContent as OpenAI.Chat.ChatCompletionUserMessageParam["content"],
				}
				merged.push(newMessage)
			}
		}
		return merged
	}, [])
}
