import { computeLineHash } from "./hash-utils"
import { ANCHOR_DELIMITER } from "./delimiter"

/**
 * Lightweight, deterministic index mapping line numbers to content hashes.
 * No Myers Diff, no persistence — content deterministically maps to hash.
 *
 * Replaces the old AnchorStateManager which used random word-pair anchors.
 */
export class FileAnchorIndex {
	/** Line index → anchor hash (after collision resolution). */
	private hashByLineIdx: string[]

	/** Anchor hash → line index (reverse lookup). */
	private lineIdxByHash: Map<string, number>

	/** The original lines (used for content comparison and gutter display). */
	private lines: string[]

	/**
	 * @param content - Array of line strings (without trailing newline).
	 */
	constructor(content: string[]) {
		this.lines = content
		this.hashByLineIdx = []
		this.lineIdxByHash = new Map()

		// Collision tracking: rawHash → list of line indices
		const collisionMap = new Map<string, number[]>()

		// First pass: compute raw hashes and detect collisions
		const rawHashes: string[] = []
		for (let i = 0; i < content.length; i++) {
			const raw = computeLineHash(content[i])
			rawHashes.push(raw)
			const existing = collisionMap.get(raw)
			if (existing) {
				existing.push(i)
			} else {
				collisionMap.set(raw, [i])
			}
		}

		// Second pass: assign final anchors with collision suffixes
		for (let i = 0; i < content.length; i++) {
			const raw = rawHashes[i]
			const indices = collisionMap.get(raw)!
			let finalHash: string
			if (indices.length === 1) {
				finalHash = raw
			} else {
				// Find position within collision group
				const collisionIndex = indices.indexOf(i)
				finalHash = `${raw}_${collisionIndex}`
			}
			this.hashByLineIdx.push(finalHash)
			this.lineIdxByHash.set(finalHash, i)
		}
	}

	/**
	 * Returns the anchor hash for the given line index.
	 */
	getHash(lineIdx: number): string {
		return this.hashByLineIdx[lineIdx]
	}

	/**
	 * Returns the line index for the given hash, or undefined if not found.
	 */
	getLineIdx(hash: string): number | undefined {
		return this.lineIdxByHash.get(hash)
	}

	/**
	 * Returns the content of the line at the given index.
	 */
	getLine(lineIdx: number): string {
		return this.lines[lineIdx]
	}

	/**
	 * Returns all lines.
	 */
	getLines(): string[] {
		return this.lines
	}

	/**
	 * After a line is modified, recompute its hash and update all maps.
	 * This is called after each edit is applied.
	 *
	 * @param lineIdx - The line index that was modified.
	 * @param newContent - The new content of the line.
	 */
	updateLine(lineIdx: number, newContent: string): void {
		const oldHash = this.hashByLineIdx[lineIdx]

		// Remove old mapping
		this.lineIdxByHash.delete(oldHash)

		// Update content
		this.lines[lineIdx] = newContent

		// Compute new hash (with collision check against all other lines)
		const raw = computeLineHash(newContent)
		let finalHash = raw
		let suffix = 0
		while (this.lineIdxByHash.has(finalHash) && this.lineIdxByHash.get(finalHash) !== lineIdx) {
			finalHash = `${raw}_${suffix}`
			suffix++
		}

		this.hashByLineIdx[lineIdx] = finalHash
		this.lineIdxByHash.set(finalHash, lineIdx)
	}

	/**
	 * Returns the gutter-formatted representation of all lines.
	 * Format: `"   42 │ a3|code..."` (5-char line number, space, box-draw, space, anchor, delimiter, content).
	 */
	getGutterRepresentation(): string[] {
		const maxLineNum = this.lines.length
		const lineNumWidth = Math.max(4, String(maxLineNum).length)
		const result: string[] = []
		for (let i = 0; i < this.lines.length; i++) {
			const lineNum = String(i + 1).padStart(lineNumWidth, " ")
			const hash = this.hashByLineIdx[i]
			const content = this.lines[i]
			result.push(`${lineNum} │ ${hash}${ANCHOR_DELIMITER}${content}`)
		}
		return result
	}

	/**
	 * Returns all anchor hashes in order (for backwards compatibility with code
	 * that expects an array of hash strings).
	 */
	getAllHashes(): string[] {
		return [...this.hashByLineIdx]
	}

	/**
	 * Number of lines in the index.
	 */
	get lineCount(): number {
		return this.lines.length
	}
}
