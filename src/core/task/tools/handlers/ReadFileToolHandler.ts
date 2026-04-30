import path from "node:path"
import fs from "node:fs/promises"
import type { ToolUse } from "@core/assistant-message"
import { formatResponse } from "@core/prompts/responses"
import { createToolError } from "@shared/tool-response"
import { resolveWorkspacePath } from "@core/workspace"
import { extractFileContent } from "@integrations/misc/extract-file-content"
import { parseContent, parseFile, generateSkeleton, ParsedDefinition } from "@services/tree-sitter"
import { loadRequiredLanguageParsers } from "@services/tree-sitter/languageParser"
import { contentHash, hashLines, stripHashes, generateFullAnchoredContent } from "@utils/line-hashing"
import { arePathsEqual, getReadablePath, isLocatedInWorkspace } from "@utils/path"
import { countFileLines, readFirstNLines } from "@utils/fs"
import { telemetryService } from "@/services/telemetry"
import { DiracSayTool } from "@/shared/ExtensionMessage"
import { DiracAssistantToolUseBlock, DiracStorageMessage, DiracUserToolResultContentBlock } from "@/shared/messages"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import { showNotificationForApproval } from "../../utils"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { ToolResultUtils } from "../utils/ToolResultUtils"

const MAX_FILE_READ_SIZE = 50 * 1024 // 50KB limit for full file reads
const OVERSIZED_FILE_PREVIEW_LINES = 200
const AUTO_EXPAND_PREVIEW_LINES = 500
const TOKEN_ESTIMATE_RATIO = 1 / 3 // approx chars per token
const SUPPORTED_EXTENSIONS = new Set(["ts", "tsx", "js", "jsx", "py", "rs", "go", "c", "cpp", "h", "hpp", "java", "php", "rb", "swift", "kt"])

interface Range {
	start: number
	end: number
}

export class ReadFileToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.FILE_READ

	constructor(private validator: ToolValidator) {}

	getDescription(block: ToolUse): string {
		const relPaths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : [])
		const range =
			block.params.start_line || block.params.end_line
				? ` lines ${block.params.start_line || 1}-${block.params.end_line || "?"}`
				: ""
		return `[${block.name} for ${relPaths.map((p) => `'${p}'`).join(", ")}${range}]`
	}

	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		const relPaths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : [])
		const config = uiHelpers.getConfig()
		if (config.isSubagentExecution) {
			return
		}

		// Create and show partial UI message
		const sharedMessageProps = {
			tool: "readFile",
			paths: relPaths.map((p) => getReadablePath(config.cwd, uiHelpers.removeClosingTag(block, "paths", p))),
			content: undefined,
			operationIsLocatedInWorkspace: (await Promise.all(relPaths.map((p) => isLocatedInWorkspace(p)))).every(Boolean),
			startLine: uiHelpers.removeClosingTag(block, "start_line", block.params.start_line),
			endLine: uiHelpers.removeClosingTag(block, "end_line", block.params.end_line),
			readFileResults: relPaths.map((p) => ({
				path: getReadablePath(config.cwd, uiHelpers.removeClosingTag(block, "paths", p)),
				status: "success" as const,
				label: "Reading...",
			})),
		}
		const partialMessage = JSON.stringify(sharedMessageProps)

		// Handle auto-approval vs manual approval for partial
		const firstPath = relPaths[0] || ""
		if (await uiHelpers.shouldAutoApproveToolWithPath(block.name, firstPath)) {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("ask", "tool")
			await uiHelpers.say("tool", partialMessage, undefined, undefined, block.partial)
		} else {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("say", "tool")
			await uiHelpers.ask("tool", partialMessage, block.partial).catch(() => {})
		}
	}

	private extractLastKnownHashFromHistory(history: DiracStorageMessage[], targetPath: string): string | undefined {
		const normalizeForComparison = (value: string): string => {
			const normalized = path.normalize(value)
			return normalized.startsWith(`.${path.sep}`) ? normalized.slice(2) : normalized
		}

		const doesPathMatch = (candidatePath: unknown): candidatePath is string => {
			if (typeof candidatePath !== "string") {
				return false
			}

			return (
				candidatePath === targetPath ||
				arePathsEqual(candidatePath, targetPath) ||
				normalizeForComparison(candidatePath) === normalizeForComparison(targetPath)
			)
		}

		// Iterate backwards to find the most recent read_file for this path, allowing for normalized equivalents
		for (let i = history.length - 1; i >= 0; i--) {
			const message = history[i]

			// Find assistant messages containing tool calls
			if (message.role === "assistant" && Array.isArray(message.content)) {
				for (const block of message.content) {
					if (block.type === "tool_use") {
						const toolUseBlock = block as unknown as DiracAssistantToolUseBlock
						const input = toolUseBlock.input as any
						const matchingPath = [input?.path, ...(Array.isArray(input?.paths) ? input.paths : [])].find((candidatePath) =>
							doesPathMatch(candidatePath),
						)
						if (toolUseBlock.name === this.name && matchingPath) {
							const toolUseId = toolUseBlock.id

							// The tool_result is almost always in the immediately following 'user' message
							const nextMessage = history[i + 1]
							if (nextMessage && nextMessage.role === "user" && Array.isArray(nextMessage.content)) {
								const resultBlock = nextMessage.content.find(
									(c) =>
										c.type === "tool_result" &&
										(c as unknown as DiracUserToolResultContentBlock).tool_use_id === toolUseId,
								)

								if (resultBlock && resultBlock.type === "tool_result") {
									// Extract text content from the result block
									const text =
										typeof resultBlock.content === "string"
											? resultBlock.content
											: Array.isArray(resultBlock.content)
												? (resultBlock.content.find((c: any) => c.type === "text") as any)?.text
												: undefined

									if (text) {
										// Match the exact hash string we output, considering potentially multiple files
										// If it's a multi-file read, we need to find the specific section for this path
										let sectionText = text
										if (text.includes(`--- ${matchingPath} ---`)) {
											const parts = text.split(`--- ${matchingPath} ---`)
											if (parts.length > 1) {
												sectionText = parts[1].split("\n--- ")[0]
											}
										}

										const match = sectionText.match(/\[File Hash: ([a-f0-9]+)\]/)
										if (match) {
											return match[1]
										}
									}
								}
							}
						}
					}
				}
			}
		}
		return undefined
	}

	private buildOutline(definitions: ParsedDefinition[]): string {
		return definitions
			.map(
				(d) =>
					`  - [${d.id}] ${d.signature || d.text.trim()} (lines ${d.fullBodyRange?.startLine || d.lineIndex + 1}-${d.fullBodyRange?.endLine || d.lineIndex + 1})`,
			)
			.join("\n")
	}

	private mergeRanges(ranges: Range[]): Range[] {
		if (ranges.length <= 1) return ranges
		const sorted = [...ranges].sort((a, b) => a.start - b.start)
		const merged: Range[] = [sorted[0]]

		for (let i = 1; i < sorted.length; i++) {
			const last = merged[merged.length - 1]
			const current = sorted[i]
			// Merge if overlapping or within 5 lines
			if (current.start <= last.end + 5) {
				last.end = Math.max(last.end, current.end)
			} else {
				merged.push(current)
			}
		}
		return merged
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const relPaths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : [])
		const startLineNum = block.params.start_line ? Number.parseInt(String(block.params.start_line)) : undefined
		const endLineNum = block.params.end_line ? Number.parseInt(String(block.params.end_line)) : undefined

		if ((block.params.start_line && isNaN(startLineNum!)) || (block.params.end_line && isNaN(endLineNum!))) {
			config.taskState.consecutiveMistakeCount++
			const error = "Invalid line numbers. Please provide valid integers for --start-line and --end-line."

			// Ensure UI is updated to mark the tool call as complete (avoiding "stuck" state)
			const sharedMessageProps = {
				tool: "readFile",
				paths: relPaths.map((p) => getReadablePath(config.cwd, p)),
				content: error,
				operationIsLocatedInWorkspace: true,
				path: relPaths[0],
				startLine: block.params.start_line?.toString(),
				endLine: block.params.end_line?.toString(),
				readFileResults: relPaths.map((p) => ({
					path: getReadablePath(config.cwd, p),
					status: "error" as const,
					label: "Invalid line numbers",
				})),
			} satisfies DiracSayTool
			const completeMessage = JSON.stringify(sharedMessageProps)

			await config.callbacks.removeLastPartialMessageIfExistsWithType("ask", "tool")
			await config.callbacks.removeLastPartialMessageIfExistsWithType("say", "tool")
			await config.callbacks.say("tool", completeMessage, undefined, undefined, false)

			return formatResponse.formatToolErrorForLLM(createToolError("tool.internalError", error, "recoverable"))
		}

		// Ensure apiConversationHistory is passed into TaskConfig from the main Dirac instance
		const history = config.messageState.getApiConversationHistory() || []

		// Extract provider information for telemetry
		const apiConfig = config.services.stateManager.getApiConfiguration()
		const currentMode = config.services.stateManager.getGlobalSettingsKey("mode")
		const provider = (currentMode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string

		// Validate required parameters
		const pathValidation = this.validator.assertRequiredParams(block, "paths")

		if (!pathValidation.ok) {
			config.taskState.consecutiveMistakeCount++
			return await config.callbacks.sayAndCreateMissingParamError(this.name, "paths")
		}

		const absolutePaths: string[] = []
		const displayPaths: string[] = []
		const workspaceContexts: any[] = []
		const results: string[] = []
		const readFileResults: any[] = []

		const imageBlocks: any[] = []
		let anyFailed = false
		let anySucceeded = false

		const supportsImages = config.api.getModel().info.supportsImages ?? false

		for (let i = 0; i < relPaths.length; i++) {
			const relPath = relPaths[i]
			const header = relPaths.length > 1 ? `--- ${relPath} ---\n` : ""

			try {
				// Resolve the absolute path
				const pathResult = resolveWorkspacePath(config, relPath, "ReadFileToolHandler.execute")
				const { absolutePath, displayPath } =
					typeof pathResult === "string" ? { absolutePath: pathResult, displayPath: relPath } : pathResult

				absolutePaths.push(absolutePath)
				displayPaths.push(displayPath)

				// Determine workspace context for telemetry
				const fallbackAbsolutePath = path.resolve(config.cwd, relPath)
				workspaceContexts.push({
					isMultiRootEnabled: config.isMultiRootEnabled || false,
					usedWorkspaceHint: typeof pathResult !== "string",
					resolvedToNonPrimary: !arePathsEqual(absolutePath, fallbackAbsolutePath),
					resolutionMethod: (typeof pathResult !== "string" ? "hint" : "primary_fallback") as
						| "hint"
						| "primary_fallback",
				})

				// 3. Exploration Parameters & Auto-Expansion
				let detail = block.params.detail as "preview" | "skeleton" | "outline" | "full" | undefined
				const maxTokens = block.params.max_tokens ? Number.parseInt(String(block.params.max_tokens)) : undefined
				const page = block.params.page as "next" | "prev" | "section" | undefined
				const section = block.params.section as string | undefined
				const rangesRaw = block.params.ranges as { start: number; end: number }[] | undefined

				const stats = await fs.stat(absolutePath)
				const ext = path.extname(absolutePath).toLowerCase()
				const isImage = [".png", ".jpg", ".jpeg", ".webp"].includes(ext)
				const fileSizeKB = stats.size / 1024

				// Auto-Expansion: increment read count
				const currentReadCount = (config.taskState.readCounts.get(absolutePath) || 0) + 1
				config.taskState.readCounts.set(absolutePath, currentReadCount)

				let effectivePreviewLines = OVERSIZED_FILE_PREVIEW_LINES
				if (currentReadCount >= 3 && !startLineNum && !endLineNum && !rangesRaw && !page) {
					effectivePreviewLines = AUTO_EXPAND_PREVIEW_LINES
				}

				// Auto-select detail if not specified
				if (!detail && !startLineNum && !endLineNum && !rangesRaw && !page) {
					detail = stats.size > MAX_FILE_READ_SIZE && !isImage ? "preview" : "full"
				}

				// 4. Handle Ranges / Pagination / Jump
				let ranges: Range[] = []
				
				if (rangesRaw && Array.isArray(rangesRaw)) {
					ranges = this.mergeRanges(rangesRaw.map(r => ({ start: Number(r.start), end: Number(r.end) })))
				} else if (page || section) {
					const currentCursor = config.taskState.fileCursors.get(absolutePath) || 1
					let finalStartLine = 1
					if (page === "next") {
						finalStartLine = currentCursor + effectivePreviewLines
					} else if (page === "prev") {
						finalStartLine = Math.max(1, currentCursor - effectivePreviewLines)
					} else if (page === "section" && section) {
						const languageParsers = await loadRequiredLanguageParsers([absolutePath])
						const definitions = await parseFile(absolutePath, languageParsers, config.services.diracIgnoreController)
						const target = definitions?.find(d => d.id === section)
						if (target) {
							finalStartLine = target.fullBodyRange?.startLine || target.lineIndex + 1
						}
					}
					ranges = [{ start: finalStartLine, end: finalStartLine + effectivePreviewLines - 1 }]
					config.taskState.fileCursors.set(absolutePath, finalStartLine)
				} else if (startLineNum || endLineNum) {
					const start = startLineNum || 1
					const end = endLineNum || start + effectivePreviewLines - 1
					ranges = [{ start, end }]
					config.taskState.fileCursors.set(absolutePath, start)
				}

				// 5. Execute based on detail level with budget awareness and diff-caching
				let usedDetail = detail || "full"
				let responseContent = ""
				let degraded = false

				const getCacheKey = (mode: string, r?: Range) => {
					return `${relPath}:${mode}:${r ? `${r.start}-${r.end}` : "full"}`
				}

				const tryGenerateContent = async (mode: string, activeRanges: Range[]): Promise<string> => {
					if (isImage) return (await extractFileContent(absolutePath, supportsImages)).text
					const cleanExt = ext.startsWith(".") ? ext.slice(1) : ext
					const isSupported = SUPPORTED_EXTENSIONS.has(cleanExt)

					switch (mode) {
						case "outline": {
							if (!isSupported) return "Outline not available for this file type."
							const languageParsers = await loadRequiredLanguageParsers([absolutePath])
							const definitions = await parseFile(absolutePath, languageParsers, config.services.diracIgnoreController)
							return definitions ? this.buildOutline(definitions) : "No definitions found."
						}
						case "skeleton": {
							if (!isSupported) return (await extractFileContent(absolutePath, false)).text
							const languageParsers = await loadRequiredLanguageParsers([absolutePath])
							const fullContent = (await extractFileContent(absolutePath, false)).text
							return await generateSkeleton(fullContent, cleanExt, languageParsers)
						}
						case "preview": {
							const preview = await readFirstNLines(absolutePath, effectivePreviewLines)
							const totalLines = await countFileLines(absolutePath)
							let chunkMap = ""
							
							if (isSupported) {
								try {
									const languageParsers = await loadRequiredLanguageParsers([absolutePath])
									const definitions = await parseFile(absolutePath, languageParsers, config.services.diracIgnoreController)
									if (definitions) {
										const filtered = definitions.filter(d => d.kind === "class" || d.kind === "function" || d.kind === "interface")
										const limited = filtered.slice(0, 50)
										chunkMap = "\nChunk map (full file):\n" + limited
											.map(d => `  - [${d.id}] ${d.name} (lines ${d.fullBodyRange?.startLine || d.lineIndex + 1}-${d.fullBodyRange?.endLine || d.lineIndex + 1})`)
											.join("\n")
										if (filtered.length > 50) {
											chunkMap += `\n  ... and ${filtered.length - 50} more symbols. Use read_file --detail outline to see all.`
										}
									}
								} catch (e) {
									// Ignore parse errors for chunk map
								}
							}
							
							const hint = formatResponse.readFilePreviewHint(relPath, totalLines, effectivePreviewLines, fileSizeKB)
							const note = currentReadCount >= 3 ? "\nNOTE: Extended preview shown due to multiple reads.\n" : ""
							return `${hashLines(preview)}${chunkMap}${note}${hint}`
						}
						case "full":
						default: {
							const fullContent = (await extractFileContent(absolutePath, supportsImages)).text
							const lines = fullContent.split(/\r?\n/)
							
							if (activeRanges.length > 0) {
								const rangeResults: string[] = []
								for (const r of activeRanges) {
									const start = Math.max(0, r.start - 1)
									const end = Math.min(lines.length, r.end)
									const slice = lines.slice(start, end)
									const anchoredSlice = generateFullAnchoredContent(slice).map((line: string) => {
										// Adjust line numbers in gutter if present
										return line.replace(/^(\s*)(\d+)/, (_: string, space: string, num: string) => {
											return space + (Number(num) + start).toString()
										})
									}).join("\n")

									const currentRangeHash = contentHash(slice.join("\n"))
									const cacheKey = getCacheKey(mode, r)
									const lastHash = config.taskState.contentHashCache.get(cacheKey)

									if (lastHash === currentRangeHash) {
										rangeResults.push(`[Lines ${r.start}-${r.end}: unchanged since your last read (Hash: ${currentRangeHash})]`)
									} else {
										config.taskState.contentHashCache.set(cacheKey, currentRangeHash)
										rangeResults.push(`[Lines ${r.start}-${r.end}, Hash: ${currentRangeHash}]\n${anchoredSlice}`)
									}
								}
								return rangeResults.join("\n\n")
							}

							// Full file
							const currentHash = contentHash(fullContent)
							const cacheKey = getCacheKey(mode)
							const lastHash = config.taskState.contentHashCache.get(cacheKey)
							
							if (lastHash === currentHash) {
								return `[Full file: unchanged since your last read (Hash: ${currentHash})]`
							}
							
							config.taskState.contentHashCache.set(cacheKey, currentHash)
							return `[File Hash: ${currentHash}]\n${hashLines(fullContent)}`
						}
					}
				}

				responseContent = await tryGenerateContent(usedDetail, ranges)

				// Budget-aware degradation (only if not already an "unchanged" response)
				if (maxTokens && !responseContent.includes("unchanged") && responseContent.length * TOKEN_ESTIMATE_RATIO > maxTokens) {
					degraded = true
					const degradationPath = ["full", "preview", "skeleton", "outline"]
					let currentIndex = degradationPath.indexOf(usedDetail)
					while (currentIndex < degradationPath.length - 1 && responseContent.length * TOKEN_ESTIMATE_RATIO > maxTokens) {
						currentIndex++
						usedDetail = degradationPath[currentIndex] as any
						responseContent = await tryGenerateContent(usedDetail, ranges)
					}
				}

				const currentHash = contentHash(responseContent)
				const tokenInfo = maxTokens ? ` [Budget: ${maxTokens}, Actual: ~${Math.round(responseContent.length * TOKEN_ESTIMATE_RATIO)}]` : ""
				const degradedInfo = degraded ? " [DEGRADED TO STAY IN BUDGET]" : ""
				
				results.push(`${header}[File Hash: ${currentHash}]${tokenInfo}${degradedInfo}\n${responseContent}`)
				
				const labelRange = ranges.length > 0 ? `lines ${ranges.map(r => `${r.start}-${r.end}`).join(", ")}` : "full"
				readFileResults.push({
					path: displayPath,
					status: "success",
					label: `Read ${usedDetail} (${labelRange})`,
				})

				await config.services.fileContextTracker.trackFileContext(relPath, "read_tool")
				anySucceeded = true
				continue
			} catch (error) {
				anyFailed = true
				const errorMessage = error instanceof Error ? error.message : String(error)
				const normalizedMessage = errorMessage.startsWith("Error reading file:")
					? errorMessage
					: `Error reading file: ${errorMessage}`
				results.push(`${header}${normalizedMessage}`)

				// Ensure arrays are filled for telemetry/UI if they haven't been yet
				if (absolutePaths.length <= i) absolutePaths.push("")
				if (displayPaths.length <= i) displayPaths.push(relPath)
				if (workspaceContexts.length <= i)
					workspaceContexts.push({ isMultiRootEnabled: !!config.isMultiRootEnabled, resolutionMethod: "error" })

				readFileResults.push({
					path: displayPaths[i] || relPath,
					status: "error",
					label: normalizedMessage,
				})
			}
		}

		if (anyFailed) {
			config.taskState.consecutiveMistakeCount++
		} else if (anySucceeded) {
			config.taskState.consecutiveMistakeCount = 0
		}

		const finalResult = results.join("\n\n")

		// Handle approval flow
		const sharedMessageProps = {
			tool: "readFile",
			paths: displayPaths.map((p) => getReadablePath(config.cwd, p)),
			content: stripHashes(finalResult, { preserveGutter: true }),
			operationIsLocatedInWorkspace: (await Promise.all(relPaths.map((p) => isLocatedInWorkspace(p)))).every(Boolean),
			path: displayPaths[0],
			startLine: startLineNum?.toString(),
			endLine: endLineNum?.toString(),
			readFileResults: readFileResults.map((r) => ({
				...r,
				path: getReadablePath(config.cwd, r.path),
			})),
		} satisfies DiracSayTool

		const completeMessage = JSON.stringify(sharedMessageProps)

		const shouldAutoApprove =
			config.isSubagentExecution ||
			(await Promise.all(relPaths.map((p) => config.callbacks.shouldAutoApproveToolWithPath(block.name, p)))).every(Boolean)

		if (shouldAutoApprove) {
			// Auto-approval flow
			if (!config.isSubagentExecution) {
				await config.callbacks.removeLastPartialMessageIfExistsWithType("ask", "tool")
				await config.callbacks.say("tool", completeMessage, undefined, undefined, false)
			}

			// Capture telemetry for each path
			for (let i = 0; i < relPaths.length; i++) {
				telemetryService.captureToolUsage(
					config.ulid,
					block.name,
					config.api.getModel().id,
					provider,
					true,
					true,
					workspaceContexts[i],
					block.isNativeToolCall,
				)
			}
		} else {
			// Manual approval flow
			const range = startLineNum || endLineNum ? ` lines ${startLineNum || 1}-${endLineNum || "?"}` : ""
			const notificationMessage = `Dirac wants to read ${relPaths.length} file(s)${range}`
			showNotificationForApproval(notificationMessage, config.autoApprovalSettings.enableNotifications)

			await config.callbacks.removeLastPartialMessageIfExistsWithType("say", "tool")

			const { didApprove } = await ToolResultUtils.askApprovalAndPushFeedback("tool", completeMessage, config)
			if (!didApprove) {
				for (let i = 0; i < relPaths.length; i++) {
					telemetryService.captureToolUsage(
						config.ulid,
						block.name,
						config.api.getModel().id,
						provider,
						false,
						false,
						workspaceContexts[i],
						block.isNativeToolCall,
					)
				}
				return formatResponse.toolDenied()
			}

			for (let i = 0; i < relPaths.length; i++) {
				telemetryService.captureToolUsage(
					config.ulid,
					block.name,
					config.api.getModel().id,
					provider,
					false,
					true,
					workspaceContexts[i],
					block.isNativeToolCall,
				)
			}
		}

		// Run PreToolUse hook after approval but before execution
		try {
			const { ToolHookUtils } = await import("../utils/ToolHookUtils")
			await ToolHookUtils.runPreToolUseIfEnabled(config, block)
		} catch (error) {
			const { PreToolUseHookCancellationError } = await import("@core/hooks/PreToolUseHookCancellationError")
			if (error instanceof PreToolUseHookCancellationError) {
				return formatResponse.toolDenied()
			}
			throw error
		}

		// Push image blocks to task state after approval
		for (const imageBlock of imageBlocks) {
			config.taskState.userMessageContent.push(imageBlock)
		}

		return finalResult
	}
}
