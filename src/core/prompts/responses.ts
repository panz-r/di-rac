import { Anthropic } from "@anthropic-ai/sdk"
import { hashLines } from "@utils/line-hashing"
import * as diff from "diff"
import * as path from "path"
import { Mode } from "../../shared/storage/types"
import { DiracIgnoreController, LOCK_TEXT_SYMBOL } from "../ignore/DiracIgnoreController"
import type { FileInfo } from "../../services/glob/list-files"
import type { ToolError } from "@shared/tool-response"

const CONTEXT_WINDOW_WARNING_THRESHOLD_PERCENT = 50

export const formatResponse = {
	duplicateFileReadNotice: () =>
		`[[NOTE] This file read has been removed to save space in the context window. Refer to the latest file read for the most up to date version of this file.]`,

	contextTruncationNotice: () =>
		`[NOTE] Some previous conversation history with the user has been removed to maintain optimal context window length. The initial user task has been retained for continuity, while intermediate conversation history has been removed. Keep this in mind as you continue assisting the user. Pay special attention to the user's latest messages.`,

	processFirstUserMessageForTruncation: () => {
		return "[Continue assisting the user!]"
	},

	condense: () =>
		`The user has accepted the condensed conversation summary you generated. This summary covers important details of the historical conversation with the user which has been truncated.\n<explicit_instructions type="condense_response">It's crucial that you respond by ONLY asking the user what you should work on next. You should NOT take any initiative or make any assumptions about continuing with work. For example you should NOT suggest file changes or attempt to read any files.\nWhen asking the user what you should work on next, you can reference information in the summary which was just generated. However, you should NOT reference information outside of what's contained in the summary for this response. Keep this response CONCISE.</explicit_instructions>`,

	toolDenied: () => `The user denied this operation.`,

	toolError: (error?: string) => `The tool execution failed with the following error:\n<error>\n${error}\n</error>`,

	readFilePreviewHint: (
		relPath: string,
		totalLines: number | undefined,
		previewLines: number,
		fileSizeKB: number,
	) => {
		const sizeInfo = `File '${relPath.toPosix()}' is ${Math.round(fileSizeKB)} KB`
		const lineInfo = totalLines !== undefined ? ` (~${totalLines.toLocaleString()} lines)` : ""
		return `\n\nNOTE: ${sizeInfo}${lineInfo}. Showing first ${previewLines} lines. To view other sections, use read_file --start-line ${previewLines + 1} --end-line ${previewLines + 200}.`
	},

	diracIgnoreError: (path: string) =>
		`Access to ${path} is blocked by the .diracignore file settings. You must try to continue in the task without using this file, or ask the user to update the .diracignore file.`,

	permissionDeniedError: (reason: string) =>
		`Command execution blocked by DIRAC_COMMAND_PERMISSIONS: ${reason}. You must try a different approach or ask the user to update the permission settings.`,

	noToolsUsed: (usingNativeToolCalls: boolean) =>
		`[ERROR] You did not use a tool in your previous response! Please retry with a tool use.


# Next Steps

If you have completed the user's task, use the attempt_completion tool. 
If you require additional information from the user, use the ask_followup_question tool. 
Otherwise, if you have not completed the task and do not need additional information, then proceed with the next step of the task. 
(This is an automated message, so do not respond to it conversationally.)`,

	tooManyMistakes: (feedback?: string) =>
		`You seem to be having trouble proceeding. The user has provided the following feedback to help guide you:\n<feedback>\n${feedback}\n</feedback>`,

	missingToolParameterError: (paramName: string) =>
		`Missing value for required parameter '${paramName}'. Please retry with complete response.\n`,

	/**
	 * Specialized error for write_to_file when the 'content' parameter is missing.
	 * Provides progressive guidance based on how many times this has happened consecutively,
	 * and includes token budget awareness to help the model understand output constraints.
	 */
	writeToFileMissingContentError: (relPath: string, consecutiveFailures: number, contextUsagePercent?: number): string => {
		const baseError = `Failed to write to '${relPath}': The --content value was empty. This typically happens when the file content is too large to generate in a single response, or when output token limits are reached before the content is fully written.`

		const contextWarning =
			contextUsagePercent !== undefined && contextUsagePercent > CONTEXT_WINDOW_WARNING_THRESHOLD_PERCENT
				? `\n\nWarning: Context window is ${contextUsagePercent}% full. The remaining output budget may be insufficient for large file writes. You MUST use a strategy that produces smaller outputs.`
				: ""

		if (consecutiveFailures >= 3) {
			// After 3+ failures, be very directive — stop trying write_to_file entirely
			return (
				`${baseError}${contextWarning}\n\n` +
				`CRITICAL: You have failed to write this file ${consecutiveFailures} times in a row. You MUST change your approach — do NOT retry write_to_file for this file again.\n\n` +
				`Required action — choose ONE of these strategies:\n` +
				`1. **Create an empty file first, then use edit_file** to add content in small sections (recommended)\n` +
				`2. **Break the file into multiple smaller files** if architecturally appropriate\n` +
				`3. **Write a minimal skeleton** using write_to_file (just imports, class/function signatures, no implementations), then use edit_file to fill in each section one at a time\n\n` +
				`Each edit_file call should target a specific part of the file.`
			)
		}
		if (consecutiveFailures >= 2) {
			// After 2 failures, strongly suggest alternative approaches
			return (
				`${baseError}${contextWarning}\n\n` +
				`This is your ${consecutiveFailures}${consecutiveFailures === 2 ? "nd" : "rd"} failed attempt. The file content is likely too large to generate in one response. You must use a different strategy:\n\n` +
				`Recommended approaches:\n` +
				`1. **Use write_to_file with a minimal skeleton** (just the structure — imports, class/function signatures, no implementations), then use edit_file to fill in each section incrementally\n` +
				`2. **Use edit_file with smaller chunks** — if the file already exists, make targeted edits instead of rewriting the entire file\n` +
				`3. **Break the task into smaller steps** — write one function or section at a time\n\n` +
				`Do NOT attempt to write the full file content in a single write_to_file call again.`
			)
		}
		// First failure — provide helpful guidance
		return (
			`${baseError}${contextWarning}\n\n` +
			`Suggestions:\n` +
			`- If the file is large, try breaking down the task into smaller steps. Write a skeleton first, then fill in sections using edit_file.\n` +
			`- If the file already exists, prefer edit_file to make targeted edits instead of rewriting the entire file.\n` +
			`- Ensure the --content value contains the complete file content before closing the tool tag.\n\n`
		)
	},

	toolResult: (
		text: string,
		images?: string[],
		fileString?: string,
	): string | Array<Anthropic.TextBlockParam | Anthropic.ImageBlockParam> => {
		const toolResultOutput = []

		if (!(images && images.length > 0) && !fileString) {
			return text
		}

		const textBlock: Anthropic.TextBlockParam = { type: "text", text }
		toolResultOutput.push(textBlock)

		if (images && images.length > 0) {
			const imageBlocks: Anthropic.ImageBlockParam[] = formatImagesIntoBlocks(images)
			toolResultOutput.push(...imageBlocks)
		}

		if (fileString) {
			const fileBlock: Anthropic.TextBlockParam = { type: "text", text: fileString }
			toolResultOutput.push(fileBlock)
		}

		return toolResultOutput
	},

	imageBlocks: (images?: string[]): Anthropic.ImageBlockParam[] => {
		return formatImagesIntoBlocks(images)
	},

	formatFilesList: (
		absolutePath: string,
		files: FileInfo[],
		didHitLimit: boolean,
		diracIgnoreController?: DiracIgnoreController,
	): string => {
		const pathMap = new Map<string, FileInfo>(files.map((f) => [f.path, f]))

		const sorted = files.sort((a, b) => {
			const aParts = a.path.split("/")
			const bParts = b.path.split("/")
			for (let i = 0; i < Math.min(aParts.length, bParts.length); i++) {
				if (aParts[i] !== bParts[i]) {
					const aPathAtLevel = aParts.slice(0, i + 1).join("/") + (i + 1 < aParts.length ? "/" : "")
					const bPathAtLevel = bParts.slice(0, i + 1).join("/") + (i + 1 < bParts.length ? "/" : "")

					const aInfo = pathMap.get(aPathAtLevel)
					const bInfo = pathMap.get(bPathAtLevel)

					if (aInfo && bInfo) {
						if (bInfo.mtime !== aInfo.mtime) {
							return bInfo.mtime - aInfo.mtime // Newest first
						}
					}

					// Fallback to alphabetical
					return aParts[i].localeCompare(bParts[i], undefined, {
						numeric: true,
						sensitivity: "base",
					})
				}
			}
			return aParts.length - bParts.length
		})

		const filtered = diracIgnoreController
			? sorted.filter((file) => diracIgnoreController.validateAccess(file.path))
			: sorted

		const formatted = filtered.map((file) => {
			let relativePath = path.relative(absolutePath, file.path).toPosix()
			if (relativePath === "" && !file.isDirectory) {
				relativePath = path.basename(file.path)
			}
			const displayPath = file.isDirectory
				? relativePath.endsWith("/")
					? relativePath
					: `${relativePath}/`
				: relativePath
			const lineCountSuffix = file.lineCount !== undefined ? ` ${file.lineCount} lines` : ""
			return `${displayPath}${lineCountSuffix}`
		})

		const note = "[Note: Files are sorted by most recently modified first within each directory.]\n\n"

		if (formatted.length === 0 || (formatted.length === 1 && formatted[0] === "")) {
			return "No files found."
		}

		const totalCount = formatted.length
		const summary = didHitLimit
			? `${totalCount} elements listed below (limit reached):`
			: `${totalCount} out of ${totalCount} elements listed below:`

		if (didHitLimit) {
			return `${note}${summary}\n\n${formatted.join(
				"\n",
			)}\n\n(File list truncated. Use list_files on specific subdirectories if you need to explore further.)`
		}

		return `${note}${summary}\n\n${formatted.join("\n")}`
	},

	createPrettyPatch: (filename = "file", oldStr?: string, newStr?: string) => {
		// strings cannot be undefined or diff throws exception
		const patch = diff.createPatch(filename.toPosix(), oldStr || "", newStr || "")
		const lines = patch.split("\n")
		const prettyPatchLines = lines.slice(4)
		return prettyPatchLines.join("\n")
	},

	taskResumption: (
		mode: Mode,
		agoText: string,
		cwd: string,
		wasRecent: boolean | 0 | undefined,
		responseText?: string,
		hasPendingFileContextWarnings?: boolean,
	): [string, string] => {
		const taskResumptionMessage = wasRecent
			? ""
			: `[TASK RESUMPTION] (${agoText}) CWD: '${cwd.toPosix()}'\n\n${
					mode === "plan"
						? "Note: Assume any previous tool use without a result failed. You are in PLAN MODE; respond to the user's message using plan_mode_respond."
						: "Note: Assume any previous tool use without a result failed. Reassess the task context and continue if incomplete."
				}`

		const userResponseMessage = responseText
			? `${mode === "plan" ? "Respond to this message" : "New instructions"}:\n<user_message>\n${responseText}\n</user_message>`
			: mode === "plan"
				? "(The user did not provide a new message. Ask how to proceed or suggest switching to Act mode.)"
				: ""

		return [taskResumptionMessage, userResponseMessage]
	},

	planModeInstructions: () => {
		return `Gather all necessary context to architect a solution before responding to the user. Once ready, present your plan for discussion. You are not allowed make code changes in this mode. If the user asks you to make changes, tell them to manually "toggle to Act mode" (you cannot switch modes yourself).`
	},

	fileEditWithUserChanges: (
		relPath: string,
		userEdits: string,
		autoFormattingEdits: string | undefined,
		newProblemsMessage: string | undefined,
	) =>
		`The user made the following updates to your content:\n\n${userEdits}\n\n` +
		(autoFormattingEdits
			? `The user's editor also applied the following auto-formatting to your content:\n\n${autoFormattingEdits}\n\n(Note: Pay close attention to changes such as single quotes being converted to double quotes, semicolons being removed or added, long lines being broken into multiple lines, adjusting indentation style, adding/removing trailing commas, etc. This will help you ensure future edit_file operations to this file are accurate.)\n\n`
			: "") +
		`The updated content, which includes both your original modifications and the additional edits, has been successfully saved to ${relPath.toPosix()}.\n\n` +
		`Please note:\n` +
		`1. You do not need to re-write the file with these changes, as they have already been applied.\n` +
		`2. Proceed with the task using this updated file state as the new baseline. (You should assume the file now contains your modifications, plus the user edits and any auto-formatting mentioned above.)\n` +
		`3. If the user's edits have addressed part of the task or changed the requirements, adjust your approach accordingly.` +
		`4. IMPORTANT: Always base your future edit_file operations on this updated file state. (If you need to verify the current file content for a future edit, you may use the read_file tool.)\n` +
		`${newProblemsMessage}`,

	fileEditWithoutUserChanges: (
		relPath: string,
		autoFormattingEdits: string | undefined,
		newProblemsMessage: string | undefined,
	) =>
		`The content was successfully saved to ${relPath.toPosix()}.\n\n` +
		(autoFormattingEdits
			? `Along with your edits, the user's editor applied the following auto-formatting to your content:\n\n${autoFormattingEdits}\n\n(Note: Pay close attention to changes such as single quotes being converted to double quotes, semicolons being removed or added, long lines being broken into multiple lines, adjusting indentation style, adding/removing trailing commas, etc. This will help you ensure future edit_file operations to this file are accurate.)\n\n`
			: "") +
		`IMPORTANT: Always base your future edit_file operations on this updated file state. (If you need to verify the current file content for a future edit, you may use the read_file tool.)\n\n` +
		`${newProblemsMessage}`,

	/** @deprecated Use edit_file instead */
	diffError: (relPath: string, originalContent: string | undefined, absolutePath?: string, ulid?: string) =>
		`This is likely because your edit could not be applied. Ensure your anchors (anchor and end_anchor) match specific, unique words that only appear on those lines. (Do NOT include the line's actual code, spaces, or braces in the anchors. Malformed XML will cause complete tool failure and break the entire editing process.)\n\n` +
		`The file was reverted to its original state:\n\n` +
		`<file_content path="${relPath.toPosix()}">\n${hashLines(originalContent || "")}\n</file_content>\n\n` +
		`Now that you have the latest state of the file, try the operation again with fewer, more precise SEARCH blocks. (If you run into this error 3 times in a row, you may use the write_to_file tool as a fallback.)`,

	diracIgnoreInstructions: (content: string) =>
		`# .diracignore\n\n(The following is provided by a root-level .diracignore file where the user has specified files and directories that should not be accessed. When using list_files, you'll notice a ${LOCK_TEXT_SYMBOL} next to files that are blocked. Attempting to access the file's contents e.g. through read_file will result in an error.)\n\n${content}\n.diracignore`,

	diracRulesGlobalDirectoryInstructions: (globalDiracRulesFilePath: string, content: string) =>
		`# .diracrules/\n\nThe following is provided by a global .diracrules/ directory, located at ${globalDiracRulesFilePath.toPosix()}, where the user has specified instructions for all working directories:\n\n${content}`,

	diracRulesLocalDirectoryInstructions: (cwd: string, content: string) =>
		`# .diracrules/\n\nThe following is provided by a root-level .diracrules/ directory where the user has specified instructions for this working directory (${cwd.toPosix()})\n\n${content}`,

	diracRulesLocalFileInstructions: (cwd: string, content: string) =>
		`# .diracrules\n\nThe following is provided by a root-level .diracrules file where the user has specified instructions for this working directory (${cwd.toPosix()})\n\n${content}`,

	windsurfRulesLocalFileInstructions: (cwd: string, content: string) =>
		`# .windsurfrules\n\nThe following is provided by a root-level .windsurfrules file where the user has specified instructions for this working directory (${cwd.toPosix()})\n\n${content}`,

	cursorRulesLocalFileInstructions: (cwd: string, content: string) =>
		`# .cursorrules\n\nThe following is provided by a root-level .cursorrules file where the user has specified instructions for this working directory (${cwd.toPosix()})\n\n${content}`,

	cursorRulesLocalDirectoryInstructions: (cwd: string, content: string) =>
		`# .cursor/rules\n\nThe following is provided by a root-level .cursor/rules directory where the user has specified instructions for this working directory (${cwd.toPosix()})\n\n${content}`,

	agentsRulesLocalFileInstructions: (cwd: string, content: string) =>
		`# AGENTS.md\n\nThe following is provided by AGENTS.md files found recursively throughout this working directory (${cwd.toPosix()}) where the user has specified instructions. Nested AGENTS.md will be combined below, and you should only apply the instructions for each AGENTS.md file that is directly applicable to the current task, i.e. if you are reading or writing to a file in that directory.\n\n${content}`,

	/**
	 * Serialize a structured ToolError into LLM-actionable prose.
	 * Each error code maps to concrete, actionable recovery guidance.
	 */
	formatToolErrorForLLM: (error: ToolError): string => {
		const guidance = formatToolErrorGuidance(error)
		const details = error.details
			? "\nAdditional context: " + Object.entries(error.details)
					.map(([k, v]) => `${k}: ${JSON.stringify(v)}`)
					.join(", ")
			: ""
		return `<tool_error severity="${error.severity}">\n${guidance}${details}\n</tool_error>`
	},

	fileContextWarning: (editedFiles: string[]): string => {

		const fileCount = editedFiles.length
		const fileVerb = fileCount === 1 ? "file has" : "files have"
		const fileDemonstrativePronoun = fileCount === 1 ? "this file" : "these files"
		const filePersonalPronoun = fileCount === 1 ? "it" : "they"

		return (
			`<explicit_instructions>\nCRITICAL FILE STATE ALERT: ${fileCount} ${fileVerb} been externally modified since your last interaction. Your cached understanding of ${fileDemonstrativePronoun} is now stale and unreliable. Before making ANY modifications to ${fileDemonstrativePronoun}, you must execute read_file to obtain the current state, as ${filePersonalPronoun} may contain completely different content than what you expect:\n` +
			`${editedFiles.map((file) => ` ${path.resolve(file).toPosix()}`).join("\n")}\n` +
			`Failure to re-read before editing will result in edit_file errors, requiring subsequent attempts and wasting tokens. You DO NOT need to re-read these files after subsequent edits, unless instructed to do so.\n</explicit_instructions>`
		)
	},
		filesystemStateNotice: (changedFiles: string[], deletedFiles: string[]): string => {
			const parts = ["[SYSTEM: Filesystem updated since session save]"]
			if (changedFiles.length > 0) {
				parts.push("The following files have been modified since this session was saved:")
				parts.push(...changedFiles.map((f) => `  ${f}`))
			}
			if (deletedFiles.length > 0) {
				parts.push("The following files have been deleted since this session was saved:")
				parts.push(...deletedFiles.map((f) => `  ${f}`))
			}
			parts.push("Any edits described in the conversation above may not reflect the current filesystem state. Re-read files before editing.")
			return parts.join("\n")
		},
}

// ── Structured ToolError → LLM Guidance ────────────────────────────────────

function formatToolErrorGuidance(error: ToolError): string {
	switch (error.code) {
		case "io.file.notFound":
			return `The specified file could not be found. Double-check the file path and try again.`
		case "io.file.permissionDenied":
			return `Permission denied when accessing the file. You do not have rights to read or write this file.`
		case "io.file.locked":
			return `The file is locked by another process. Wait a moment and retry the operation.`
		case "anchor.notFound":
			return `One or more line anchors could not be found in the current file content. The file may have been modified externally — re-read the file to obtain current anchors, then retry the edit with updated anchors.`
		case "anchor.contentMismatch":
			return `The content at an anchor line has changed since you last read the file. Re-read the file to get the current state and anchors, then retry the edit.`
		case "anchor.invalidFormat":
			return `An anchor was incorrectly formatted. Anchors must follow the format "content" (e.g., "code"). Check your edit parameters and retry.`
		case "edit.multiFileConflict":
			return `Conflicting edits were detected across multiple files. Ensure your edits do not overlap and retry each conflicting file separately.`
		case "lsp.timeout":
			return `A language server operation timed out. Retry the operation — if it persists, try a different approach.`
		case "lsp.connectionLost":
			return `The language server connection was lost. Retry the operation; the connection may recover automatically.`
		case "manifest.invalidSchema":
			return `The operation manifest had an invalid structure. Check the manifest format and retry.`
		case "manifest.duplicateOp":
			return `A duplicate operation was detected in the manifest. Remove the duplicate and retry.`
		case "manifest.orderingConflict":
			return `Operation ordering conflicts were detected in the manifest. Reorder the operations to resolve dependencies.`
		case "speculative.workspace.rejected":
			return `A speculative workspace change was rejected. The primary workspace state is unchanged — continue with the current approach.`
		case "speculative.verify.failed":
			return `Speculative verification failed. The proposed changes may have issues — review and adjust before retrying.`
		case "arg.invalidArgument":
			return `One or more arguments you provided were of the wrong type or format. ${error.message ? error.message + " " : ""}Check the parameter types and try again with corrected values.`
		case "tool.unknownError":
			return `An unexpected error occurred during tool execution. ${error.message ? error.message + " " : ""}Try a different approach or re-read relevant files to ensure your context is up to date.`
		case "tool.internalError":
			return `An internal error occurred in the tool infrastructure. ${error.message ? error.message + " " : ""}This is not caused by your action — retry the operation, or try a different approach to accomplish the same goal.`
		default:
			return `Tool execution failed${error.message ? ": " + error.message : ""}. Try a different approach or check your inputs.`
	}
}

// to avoid circular dependency
const formatImagesIntoBlocks = (images?: string[]): Anthropic.ImageBlockParam[] => {
	return images
		? images.map((dataUrl) => {
				// data:image/png;base64,base64string
				const [rest, base64] = dataUrl.split(",")
				const mimeType = rest.split(":")[1].split(";")[0]
				return {
					type: "image",
					source: {
						type: "base64",
						media_type: mimeType,
						data: base64,
					},
				} as Anthropic.ImageBlockParam
			})
		: []
}
