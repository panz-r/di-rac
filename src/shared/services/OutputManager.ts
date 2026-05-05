/**
 * Disk-backed output manager for large tool results.
 *
 * When tool outputs exceed a threshold, saves the full content to
 * `.dirac/outputs/<tool>_<timestamp>.txt` and returns a reference
 * pointer + preview for the conversation context.
 */

import { mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs"
import { join } from "node:path"

const DEFAULT_THRESHOLD = 4 * 1024 // 4KB
const DEFAULT_PREVIEW_SIZE = 2 * 1024 // 2KB
const MAX_OUTPUT_AGE_MS = 24 * 60 * 60 * 1000 // 24 hours

export class OutputManager {
	private readonly outputsDir: string

	constructor(workspaceRoot: string) {
		this.outputsDir = join(workspaceRoot, ".dirac", "outputs")
		try {
			mkdirSync(this.outputsDir, { recursive: true })
		} catch {
			// Directory may already exist
		}
		this.cleanupOldOutputs()
	}

	/**
	 * Save full output to disk if it exceeds the threshold.
	 * Returns a reference string + preview for context, or null if below threshold.
	 */
	saveOutput(toolName: string, content: string, threshold = DEFAULT_THRESHOLD): { reference: string; preview: string; filename: string } | null {
		if (content.length < threshold) {
			return null
		}

		const timestamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19)
		const filename = `${toolName}_${timestamp}.txt`
		const filePath = join(this.outputsDir, filename)

		try {
			writeFileSync(filePath, content, "utf8")
		} catch {
			// If write fails, return null so caller uses original content
			return null
		}

		const sizeKB = (content.length / 1024).toFixed(1)
		const reference = `[Output saved to .dirac/outputs/${filename} (${sizeKB}KB)]`
		const preview = content.slice(0, DEFAULT_PREVIEW_SIZE)

		return { reference, preview, filename }
	}

	/**
	 * Get the outputs directory path.
	 */
	getOutputsDir(): string {
		return this.outputsDir
	}

	/**
	 * Enforce a per-message budget. If content exceeds maxBytes,
	 * save to disk and return reference + preview.
	 */
	enforceBudget(content: string, maxBytes: number, toolName = "tool"): string {
		const result = this.saveOutput(toolName, content, maxBytes)
		if (result) {
			return `${result.reference}\n\n${result.preview}\n\n--- [Output truncated. Use bash to view: bash "cat .dirac/outputs/${result.filename}"] ---`
		}
		return content
	}

	/**
	 * List all saved output files.
	 */
	listOutputs(): { filename: string; sizeKB: string; modified: string }[] {
		try {
			const files = readdirSync(this.outputsDir)
			return files
				.filter((f) => f.endsWith(".txt"))
				.map((f) => {
					const filePath = join(this.outputsDir, f)
					try {
						const stats = statSync(filePath)
						return {
							filename: f,
							sizeKB: (stats.size / 1024).toFixed(1),
							modified: stats.mtime.toISOString(),
						}
					} catch {
						return null
					}
			})
				.filter(Boolean) as { filename: string; sizeKB: string; modified: string }[]
		} catch {
			return []
		}
	}

	/**
	 * Read a saved output file by filename.
	 */
	readOutput(filename: string): string | null {
		try {
			const filePath = join(this.outputsDir, filename)
			return readFileSync(filePath, "utf8")
		} catch {
			return null
		}
	}

	/**
	 * Delete all saved output files.
	 */
	clearOutputs(): number {
		let count = 0
		try {
			const files = readdirSync(this.outputsDir)
			for (const f of files) {
				if (f.endsWith(".txt")) {
					rmSync(join(this.outputsDir, f))
					count++
				}
			}
		} catch {
			// Directory may not exist
		}
		return count
	}

	/**
	 * Remove output files older than 24 hours.
	 */
	private cleanupOldOutputs(): void {
		try {
			const files = readdirSync(this.outputsDir)
			const now = Date.now()
			for (const file of files) {
				const filePath = join(this.outputsDir, file)
				try {
					const stats = statSync(filePath)
					if (now - stats.mtimeMs > MAX_OUTPUT_AGE_MS) {
						rmSync(filePath)
					}
				} catch {
					// Skip files that can't be stat'd
				}
			}
		} catch {
			// Directory may not exist yet
		}
	}
}
