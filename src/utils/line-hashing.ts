export { ANCHOR_DELIMITER, extractId, getDelimiter, stripHashes } from "../shared/utils/line-hashing"

import { ANCHOR_DELIMITER } from "../shared/utils/line-hashing"
import { computeLineHash } from "../shared/utils/hash-utils"
import { FileAnchorIndex } from "../shared/utils/file-anchor-index"

/**
 * Generates a content-based hash for a single line.
 * Delegates to computeLineHash from hash-utils.ts.
 */
export function hashLine(line: string): string {
	return computeLineHash(line)
}

export function formatAnchoredLine(line: string, hash: string, lineNum?: number, gutterWidth: number = 4): string {
	if (lineNum === undefined) {
		return `${hash}${ANCHOR_DELIMITER}${line}`
	}
	const paddedNum = String(lineNum).padStart(gutterWidth, " ")
	return `${paddedNum} │ ${hash}${ANCHOR_DELIMITER}${line}`
}

// Keep backwards-compatible export name
export { formatAnchoredLine as formatLineWithHash }

/**
 * Parses an anchored line returned by the LLM and extracts the hash and content.
 * Handles the optional gutter prefix ("   42 │ ").
 *
 * @param anchoredLine - The anchored line string (e.g., "   42 │ a3|def foo()" or just "a3|def foo()")
 * @returns Object with hash and content, or null if parsing fails
 */
export function parseAnchorFromLine(anchoredLine: string): { hash: string; content: string } | null {
	if (!anchoredLine) {
		return null
	}
	const trimmed = anchoredLine.trim()

	// Pattern: optional gutter number + separator, then hash + delimiter + content
	// First try to strip the gutter: "42 │ a3|content" → "a3|content"
	let afterGutter = trimmed
	const gutterMatch = trimmed.match(/^\d+\s*[│|]\s*/)
	if (gutterMatch) {
		afterGutter = trimmed.substring(gutterMatch[0].length)
	}

	const delimiterIndex = afterGutter.indexOf(ANCHOR_DELIMITER)
	if (delimiterIndex === -1) {
		// No delimiter found — if it looks like just a hash, treat as hash with empty content
		const potentialHash = afterGutter.trim()
		if (/^[a-z0-9_]{2,5}$/.test(potentialHash)) {
			return { hash: potentialHash, content: "" }
		}
		return null
	}

	const hash = afterGutter.substring(0, delimiterIndex)
	const content = afterGutter.substring(delimiterIndex + ANCHOR_DELIMITER.length)

	// Validate hash format
	if (!/^[a-z0-9_]{2,5}$/.test(hash)) {
		return null
	}

	return { hash, content }
}

/**
 * Splits an anchored line into anchor and content.
 * Backwards compatible wrapper around parseAnchorFromLine.
 */
export function splitAnchor(rawAnchor: string): { anchor: string; content: string } {
	const result = parseAnchorFromLine(rawAnchor)
	if (!result) {
		// Fallback: return trimmed anchor, empty content
		return { anchor: rawAnchor.trim(), content: "" }
	}
	return { anchor: result.hash, content: result.content }
}

/**
 * Generates fully anchored content for a whole file using content-hash anchors.
 * Uses FileAnchorIndex to build deterministic, content-based anchors.
 *
 * @param content - Array of line strings
 * @returns Array of formatted lines with gutter, anchors, and delimiters
 */
export function generateFullAnchoredContent(content: string[]): string[] {
	const index = new FileAnchorIndex(content)
	return index.getGutterRepresentation()
}

/**
 * Hashes all lines in a given content string using content-based anchors.
 * This replaces the old stateful anchor system with deterministic hashes.
 *
 * @param content - The full text content to hash
 * @returns The content with each line prefixed by its gutter, anchor, and delimiter
 */
export function hashLines(content: string): string {
	if (!content) {
		return ""
	}

	const lines = content.split(/\r?\n/)
	return generateFullAnchoredContent(lines).join("\n")
}

/**
 * Generates a 32-bit FNV-1a hash for the given content string.
 * Used for whole-file content comparison (cheap change detection).
 *
 * @param content - The text content to hash
 * @returns An 8-character hex string representing the hash
 */
export function contentHash(content: string): string {
	let h = 2166136261 // FNV-1a offset basis
	for (let i = 0; i < content.length; i++) {
		h = Math.imul(h ^ content.charCodeAt(i), 16777619) // FNV-1a prime
	}
	return (h >>> 0).toString(16).padStart(8, "0")
}
