/**
 * Shared utility for hash-anchored line protocol.
 * Used by both the extension (to generate/reconcile hashes) and the webview (to strip hashes for display).
 */

export { ANCHOR_DELIMITER } from "./delimiter"
import { ANCHOR_DELIMITER } from "./delimiter"

/**
 * Returns the centralized delimiter used to separate anchors from content.
 *
 * @returns The anchor delimiter string
 */
export function getDelimiter(): string {
	return ANCHOR_DELIMITER
}

/**
 * Helper to escape characters for use in a regular expression.
 */
function escapeRegExp(string: string) {
	return string.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}

/**
 * Strips hash anchors from a content string.
 * Removes patterns like "a3|" (lowercase alphanumeric + optional underscore + delimiter)
 * from the beginning of each line.
 *
 * @param content - The content containing hashed lines
 * @returns The clean content without hashes
 */
export function stripHashes(content: string): string {
	if (!content) {
		return ""
	}

	// Match content-hash anchors: 2-5 lowercase alphanumeric + underscore chars followed by delimiter.
	// Anchors appear at the start of a line (after optional gutter like "   42 | ").
	const delimiterRegex = new RegExp(`(?:^\\s*\\d+\\s*[│|]\\s*)?[a-z0-9_]{2,5}${escapeRegExp(ANCHOR_DELIMITER)}`, "gm")
	return content.replace(delimiterRegex, "")
}

/**
 * Extracts the ID (anchor hash) from a line reference provided by the model.
 * Handles both "a3" and "a3|content" formats.
 *
 * @param ref - The line reference string
 * @returns The extracted ID
 */
export function extractId(ref: string): string {
	if (!ref) {
		return ""
	}
	const delimiterIndex = ref.indexOf(ANCHOR_DELIMITER)
	return delimiterIndex === -1 ? ref.trim() : ref.substring(0, delimiterIndex).trim()
}
