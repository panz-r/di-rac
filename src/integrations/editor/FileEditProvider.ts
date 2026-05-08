import { DiffViewProvider } from "@integrations/editor/DiffViewProvider"
import * as fs from "fs/promises"
import * as fsSync from "fs"
import { Logger } from "@/shared/services/Logger"

/**
 * A file-system-based implementation of DiffViewProvider that performs direct file operations
 * without visual editor integration. This provider uses the Node.js fs package to handle
 * file edits in-memory and then writes them to disk.
 *
 * Visual operations like scrolling are implemented as no-ops since there is no UI component.
 * This makes it suitable for headless or non-interactive environments.
 */
export class FileEditProvider extends DiffViewProvider {
	private documentContent?: string

	constructor() {
		super()
	}

	override showFile(_absolutePath: string): Promise<void> {
		// No-op: No visual editor to show the file
		return Promise.resolve()
	}

	protected async openDiffEditor(): Promise<void> {
		// No-op: No visual editor to open in a file-system-only provider
		// The file content is already loaded in the base class's open() method
		this.documentContent = this.originalContent || ""
	}

	override async open(relPath: string, options?: { displayPath?: string }): Promise<void> {
		await super.open(relPath, options)
		this.documentContent = this.originalContent || ""
	}

	async replaceText(
		content: string,
		rangeToReplace: { startLine: number; endLine: number },
		_currentLine: number | undefined,
	): Promise<void> {
		if (this.documentContent === undefined) {
			throw new Error(`Document not initialized for ${this.relPath || "unknown file"}. This can happen if the file failed to open or was already reset.`)
		}

		// Split the document into lines
		const lines = this.documentContent.split("\n")

		// Check if we're replacing to the end of the document
		const replacingToEnd = rangeToReplace.endLine >= lines.length

		// Replace the specified range with the new content
		const newContentLines = content.split("\n")

		// Remove trailing empty line for proper splicing, BUT only when NOT replacing
		// to the end of the document. When replacing to the end, keep the trailing
		// empty string to preserve trailing newlines from the content.
		if (!replacingToEnd && newContentLines[newContentLines.length - 1] === "") {
			newContentLines.pop()
		}

		// Splice the lines array to replace the range
		lines.splice(rangeToReplace.startLine, rangeToReplace.endLine - rangeToReplace.startLine, ...newContentLines)

		// Join the lines back together
		this.documentContent = lines.join("\n")
	}

	protected async scrollEditorToLine(_line: number): Promise<void> {
		// No-op: No visual editor to scroll
	}

	protected async scrollAnimation(_startLine: number, _endLine: number): Promise<void> {
		// No-op: No visual editor to animate
	}

	protected async truncateDocument(lineNumber: number): Promise<void> {
		if (!this.documentContent) {
			return
		}

		// Split the document into lines and keep only up to lineNumber
		const lines = this.documentContent.split("\n")
		if (lineNumber < lines.length) {
			this.documentContent = lines.slice(0, lineNumber).join("\n")
		}
	}

	protected async getDocumentLineCount(): Promise<number> {
		if (!this.documentContent) {
			return 0
		}
		return this.documentContent.split("\n").length
	}

	protected async getDocumentText(): Promise<string | undefined> {
		return this.documentContent
	}

	/**
	 * Public method to get the current document content.
	 * This is exposed for use by tools that need to read the document state.
	 */
	public async getContent(): Promise<string | undefined> {
		return this.getDocumentText()
	}

	protected async saveDocument(): Promise<boolean> {
		if (!this.absolutePath || !this.documentContent) {
			return false
		}

		try {
			const targetPath = this.absolutePath
			const newContent = this.documentContent

			// Idempotency: skip write if content matches existing file (after normalization)
			// Trailing whitespace normalization is skipped for whitespace-sensitive files (Python, YAML, Makefiles)
			if (fsSync.existsSync(targetPath)) {
				const existing = await fs.readFile(targetPath, "utf8")
				const wsSensitive = /(\.(py|hs|lhs|yaml|yml|mak)$|Makefile$|Makefile\.)/i.test(targetPath)
				const normalize = (s: string) => {
					let n = s.replace(/\r\n/g, "\n")
					if (!wsSensitive) n = n.replace(/[ \t]+$/gm, "")
					return n
				}
				if (normalize(existing) === normalize(newContent)) {
					this.editType = undefined
					return true
				}
			}

			// Atomic write: temp-file + rename (POSIX atomic, prevents partial-write corruption)
			const tempPath = `${targetPath}.tmp.${Date.now()}`
			try {
				await fs.writeFile(tempPath, newContent, { encoding: "utf8" })
				await fs.rename(tempPath, targetPath)
			} catch (renameError) {
				// Clean up temp file if rename fails
				try {
					await fs.unlink(tempPath)
				} catch {}
				throw renameError
			}
			return true
		} catch (error) {
			Logger.error(`Failed to save document to ${this.absolutePath}:`, error)
			return false
		}
	}

	protected async closeAllDiffViews(): Promise<void> {
		// No-op: No visual diff views to close
	}

	protected async resetDiffView(): Promise<void> {
		// Clean up the in-memory document content
		this.documentContent = undefined
	}
	override async applyAndSaveSilently(
		absolutePath: string,
		content: string,
	): Promise<{
		finalContent: string | undefined
		autoFormattingEdits: string | undefined
		userEdits: string | undefined
	}> {
		await this.open(absolutePath)
		const range = { startLine: 0, endLine: await this.getDocumentLineCount() }
		await this.replaceText(content, range, undefined)
		await this.saveDocument()

		const finalContent = await this.getDocumentText()

		return {
			finalContent,
			autoFormattingEdits: undefined,
			userEdits: undefined,
		}
	}


}
