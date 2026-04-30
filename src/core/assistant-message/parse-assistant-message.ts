import { AssistantMessageContent } from "."

/**
 * Parses an assistant message string into content blocks.
 * Supports `<thinking>` and `<think>` tags for reasoning blocks.
 * Assumes native tool calling, so no XML tool call parsing is performed.
 *
 * @param assistantMessage The raw string output from the assistant.
 * @param isPartial Whether this is a partial message (streaming).
 * @returns An array of `AssistantMessageContent` objects, which can be `TextContent` or `ReasoningContent`.
 */
export function parseAssistantMessageV2(assistantMessage: string, isPartial?: boolean): AssistantMessageContent[] {
	const contentBlocks: AssistantMessageContent[] = []
	let i = 0
	const len = assistantMessage.length

	// Pattern for matching reasoning tags (opening or closing)
	const searchPattern = new RegExp("<(thinking|think)>|</(thinking|think)>", "gi")

	while (i < len) {
		const remaining = assistantMessage.slice(i)

		// Check for reasoning tags
		const openingTagMatch = remaining.match(/^<(thinking|think)>/i)
		if (openingTagMatch) {
			const openingTag = openingTagMatch[0]
			
			// Phase 15 Hardening: Search for the matching closing tag by tracking nesting depth
			// This handles nested tags and mismatched think/thinking tags robustly.
			searchPattern.lastIndex = openingTag.length
			
			let depth = 1
			let closingTagIndex = -1
			let finalClosingTagLen = 0
			let match: RegExpExecArray | null

			while ((match = searchPattern.exec(remaining)) !== null) {
				if (match[0].startsWith("</")) {
					depth--
					if (depth === 0) {
						closingTagIndex = match.index
						finalClosingTagLen = match[0].length
						break
					}
				} else {
					depth++
				}
			}

			if (closingTagIndex !== -1) {
				// Found complete reasoning block
				const content = remaining.slice(openingTag.length, closingTagIndex)
				if (content) {
					contentBlocks.push({
						type: "reasoning",
						reasoning: content,
						partial: false,
					})
				}
				i += closingTagIndex + finalClosingTagLen
				continue
			}
			// Partial reasoning block (tag not closed)
			const content = remaining.slice(openingTag.length)
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
		} else {
			// No more tags, finalize remaining as text
			const text = remaining.trim()
			if (text) {
				contentBlocks.push({
					type: "text",
					content: text,
					partial: isPartial ?? true,
				})
			}
			i = len
		}
	}

	return contentBlocks
}
