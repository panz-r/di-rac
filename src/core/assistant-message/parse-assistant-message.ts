import { AssistantMessageContent } from "."

/**
 * Parses an assistant message string into content blocks.
 * Supports `<thinking>` and `<think>` tags for reasoning blocks.
 * Assumes native tool calling, so no XML tool call parsing is performed.
 *
 * @param assistantMessage The raw string output from the assistant.
 * @returns An array of `AssistantMessageContent` objects, which can be `TextContent` or `ReasoningContent`.
 */
export function parseAssistantMessageV2(assistantMessage: string): AssistantMessageContent[] {
	const contentBlocks: AssistantMessageContent[] = []
	let i = 0
	const len = assistantMessage.length

	while (i < len) {
		const remaining = assistantMessage.slice(i)

		// Check for reasoning tags
		const openingTagMatch = remaining.match(/^<(thinking|think)>/i)
		if (openingTagMatch) {
			const tagName = openingTagMatch[1]
			const openingTag = openingTagMatch[0]
			
			// Phase 15 Hardening: Search for both possible closing tags to handle mismatches
			const closingTag1 = `</${tagName}>`
			const closingTag2 = tagName.toLowerCase() === "thinking" ? "</think>" : "</thinking>"
			
			const idx1 = remaining.toLowerCase().indexOf(closingTag1.toLowerCase(), openingTag.length)
			const idx2 = remaining.toLowerCase().indexOf(closingTag2.toLowerCase(), openingTag.length)
			
			const closingTagIndex = idx1 === -1 ? idx2 : idx2 === -1 ? idx1 : Math.min(idx1, idx2)
			const finalClosingTag = closingTagIndex === idx1 ? closingTag1 : closingTag2

			if (closingTagIndex !== -1) {
				// Found complete reasoning block
				const content = remaining.slice(openingTag.length, closingTagIndex).trim()
				if (content) {
					contentBlocks.push({
						type: "reasoning",
						reasoning: content,
						partial: false,
					})
				}
				i += closingTagIndex + finalClosingTag.length
				continue
			}
			// Partial reasoning block (tag not closed)
			const content = remaining.slice(openingTag.length).trim()
			if (content) {
				contentBlocks.push({
					type: "reasoning",
					reasoning: content,
					partial: true,
				})
			}
			i = len // Done
			continue
		}

		// It's a text block or we're looking for the next reasoning tag
		const nextTagMatch = remaining.match(/<(thinking|think)>/i)
		if (nextTagMatch && nextTagMatch.index !== undefined && nextTagMatch.index > 0) {
			// Found a tag later in the string, finalize text until then
			const text = remaining.slice(0, nextTagMatch.index).trim()
			if (text) {
				contentBlocks.push({
					type: "text",
					content: text,
					partial: false,
				})
			}
			i += nextTagMatch.index
		} else if (!nextTagMatch) {
			// No more tags, finalize remaining as text
			const text = remaining.trim()
			if (text) {
				contentBlocks.push({
					type: "text",
					content: text,
					partial: true,
				})
			}
			i = len
		} else if (nextTagMatch.index === 0) {
			// Should have been handled by the openingTagMatch above, but just in case
			// If it's a tag we don't recognize or malformed, treat as text
			const text = remaining[0]
			if (contentBlocks.length > 0 && contentBlocks[contentBlocks.length - 1].type === "text") {
				;(contentBlocks[contentBlocks.length - 1] as any).content += text
			} else {
				contentBlocks.push({
					type: "text",
					content: text,
					partial: true,
				})
			}
			i++
		}
	}

	return contentBlocks
}
