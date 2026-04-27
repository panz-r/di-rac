import { ToolUse } from "@core/assistant-message"
import { parseAnchorFromLine, stripHashes } from "@utils/line-hashing"
import { ANCHOR_DELIMITER } from "@shared/utils/delimiter"
import { FileAnchorIndex } from "@shared/utils/file-anchor-index"
import { AppliedEdit, Edit, FailedEdit, ResolvedEdit } from "./types"

const WHITESPACE_SENSITIVE_EXTENSIONS = new Set([
	".py", ".hs", ".lhs", ".yaml", ".yml", "Makefile", ".mak",
])

const MAX_SUGGESTION_DISTANCE = 2
const MAX_SUGGESTIONS = 3

function isWhitespaceSensitive(filePath: string): boolean {
	if (filePath.endsWith("Makefile") || filePath === "Makefile") return true
	const lastDot = filePath.lastIndexOf(".")
	if (lastDot === -1) return false
	return WHITESPACE_SENSITIVE_EXTENSIONS.has(filePath.substring(lastDot).toLowerCase())
}

function levenshtein(a: string, b: string): number {
	const m = a.length, n = b.length
	const dp: number[][] = []
	for (let i = 0; i <= m; i++) { dp[i] = [i] }
	for (let j = 0; j <= n; j++) { dp[0][j] = j }
	for (let i = 1; i <= m; i++) {
		for (let j = 1; j <= n; j++) {
			dp[i][j] = a[i-1] === b[j-1] ? dp[i-1][j-1] : 1 + Math.min(dp[i-1][j], dp[i][j-1], dp[i-1][j-1])
		}
	}
	return dp[m][n]
}

function normalizeWhitespace(s: string): string {
	return s.replace(/\s+$/, "").replace(/\t/g, "    ")
}

function truncate(s: string, maxLen: number = 60): string {
	return s.length <= maxLen ? s : s.substring(0, maxLen) + "..."
}

export class EditExecutor {
	resolveEdits(
		blocks: ToolUse[],
		lines: string[],
		lineHashes: string[],
		fileAnchorIndex?: FileAnchorIndex,
		filePath?: string,
	): { resolvedEdits: ResolvedEdit[]; failedEdits: FailedEdit[] } {
		const failedEdits: FailedEdit[] = []
		const resolvedEdits: ResolvedEdit[] = []
		const index = fileAnchorIndex || new FileAnchorIndex(lines)
		const langIsWhitespaceSensitive = filePath ? isWhitespaceSensitive(filePath) : false

		for (const block of blocks) {
			const edits = (block.params.edits as Edit[]) || []
			for (const edit of edits) {
				const diagnostics: string[] = []
				const editType = edit.edit_type

				const { index: lineIdx, error: startError } = this.resolveAnchor(
					"anchor", edit.anchor, index, langIsWhitespaceSensitive)
				if (startError) diagnostics.push(startError)

				let endIdx = lineIdx
				if (editType === "replace") {
					const { index: resolvedEndIdx, error: endError } = this.resolveAnchor(
						"end_anchor", edit.end_anchor, index, langIsWhitespaceSensitive)
					if (endError) diagnostics.push(endError)
					endIdx = resolvedEndIdx
				}

				if (lineIdx !== -1 && endIdx !== -1 && endIdx < lineIdx) {
					diagnostics.push("Range error: anchor must refer to a line that precedes or is the same as end_anchor.")
				}

				if (diagnostics.length > 0) {
					failedEdits.push({ edit, error: diagnostics.join(" ") })
				} else {
					resolvedEdits.push({ lineIdx, endIdx, edit })
				}
			}
		}
		return { resolvedEdits, failedEdits }
	}

	resolveAnchor(
		type: "anchor" | "end_anchor",
		rawAnchor: string | undefined,
		index: FileAnchorIndex,
		langIsWhitespaceSensitive: boolean,
	): { index: number; error?: string } {
		const anchorRaw = rawAnchor || ""
		if (!anchorRaw.trim()) return { index: -1, error: `${type} is missing.` }

		const parsed = parseAnchorFromLine(anchorRaw)
		if (!parsed) {
			return {
				index: -1,
				error: `${type} is missing or incorrectly formatted. It must follow the format "hash${ANCHOR_DELIMITER}content" (e.g., "a3${ANCHOR_DELIMITER}code").`,
			}
		}

		const { hash, content: providedContentRaw } = parsed
		const lineIdx = index.getLineIdx(hash)

		if (lineIdx === undefined) {
			return { index: -1, error: this.buildUnknownAnchorError(hash, index) }
		}

		// Echo stripping
		const echoPattern = new RegExp(`^[a-z0-9_]{1,32}\\${ANCHOR_DELIMITER}`)
		let providedContent = providedContentRaw.replace(echoPattern, "")

		if (providedContent.includes("\n") || providedContent.includes("\r")) {
			return {
				index: -1,
				error: `${type} "${hash}" exists, but the provided code line contains a newline character. Use format hash${ANCHOR_DELIMITER}{line_text}.`,
			}
		}

		const actualContent = index.getLine(lineIdx)
		let providedCmp = providedContent
		let actualCmp = actualContent

		if (!langIsWhitespaceSensitive) {
			providedCmp = normalizeWhitespace(providedContent)
			actualCmp = normalizeWhitespace(actualContent)
		}

		if (providedCmp !== actualCmp) {
			return {
				index: -1,
				error: `Anchor "${hash}" is stale. Current content: '${truncate(actualContent)}' with new anchor ${index.getHash(lineIdx)}.`,
			}
		}

		return { index: lineIdx }
	}

	private buildUnknownAnchorError(hash: string, index: FileAnchorIndex): string {
		const allHashes = index.getAllHashes()
		const scored = allHashes.map((h) => ({
			hash: h,
			distance: levenshtein(hash, h),
			lineIdx: index.getLineIdx(h)!,
		}))
		scored.sort((a, b) => a.distance - b.distance || a.lineIdx - b.lineIdx)

		const suggestions = scored
			.filter((s) => s.distance <= MAX_SUGGESTION_DISTANCE)
			.slice(0, MAX_SUGGESTIONS)

		if (suggestions.length > 0) {
			const suggestionList = suggestions
				.map((s) => `"${s.hash}" (line ${s.lineIdx + 1}): '${truncate(index.getLine(s.lineIdx), 40)}'`)
				.join(", ")
			return `Anchor "${hash}" not found. Did you mean: ${suggestionList}?`
		}

		return `Anchor "${hash}" not found in the file. Please re-read the file to get current anchors.`
	}

	applyEdits(
		lines: string[],
		resolvedEdits: ResolvedEdit[],
		fileAnchorIndex?: FileAnchorIndex,
	): { finalLines: string[]; addedCount: number; removedCount: number; appliedEdits: AppliedEdit[] } {
		const sortedEdits = [...resolvedEdits].sort((a, b) => b.lineIdx - a.lineIdx)
		const newLines = [...lines]
		let addedCount = 0
		let removedCount = 0
		const changes: Array<{ originalLineIdx: number; replacementCount: number; removedCount: number; edit: Edit }> = []

		for (const { lineIdx, endIdx, edit } of sortedEdits) {
			const editType = edit.edit_type
			const cleanText = stripHashes(edit.text || "")
			const replacementLines = cleanText === "" ? [] : cleanText.split(/\r?\n/)

			let removedInThisEdit: number
			let spliceIndex: number

			if (editType === "insert_after") {
				spliceIndex = lineIdx + 1
				removedInThisEdit = 0
			} else if (editType === "insert_before") {
				spliceIndex = lineIdx
				removedInThisEdit = 0
			} else {
				spliceIndex = lineIdx
				removedInThisEdit = endIdx - lineIdx + 1
			}

			newLines.splice(spliceIndex, removedInThisEdit, ...replacementLines)
			addedCount += replacementLines.length
			removedCount += removedInThisEdit
			changes.push({ originalLineIdx: lineIdx, replacementCount: replacementLines.length, removedCount: removedInThisEdit, edit })

			if (fileAnchorIndex) {
				for (let i = 0; i < replacementLines.length; i++) {
					const targetLineIdx = spliceIndex + i
					if (targetLineIdx < newLines.length) {
						fileAnchorIndex.updateLine(targetLineIdx, replacementLines[i])
					}
				}
			}
		}

		const appliedEdits: AppliedEdit[] = changes.map((change) => {
			let shift = 0
			for (const other of changes) {
				if (other.originalLineIdx < change.originalLineIdx) {
					shift += other.replacementCount - other.removedCount
				}
			}
			return {
				startIdx: change.originalLineIdx + shift,
				endIdx: change.originalLineIdx + shift + change.replacementCount - 1,
				originalStartIdx: change.originalLineIdx,
				originalEndIdx: change.originalLineIdx + change.removedCount - 1,
				edit: change.edit,
				linesAdded: change.replacementCount,
				linesDeleted: change.removedCount,
			}
		})

		return { finalLines: newLines, addedCount, removedCount, appliedEdits }
	}

	formatFailureMessage(edit: Edit, error?: string): string {
		const diagnostic = error
			? ` Diagnostics: ${error}`
			: " This almost certainly is because the anchors used were incorrect or not in ascending order or the text supplied was incorrect. Please check again before editing."
		return `Edit (anchor: "${edit.anchor}", end_anchor: "${edit.end_anchor}") failed.${diagnostic}`
	}
}
