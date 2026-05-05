import { ToolUse } from "@core/assistant-message"
import { formatResponse } from "@core/prompts/responses"
import { createToolError } from "@shared/tool-response"

import { DiracDefaultTool } from "@/shared/tools"
import { ToolResponse } from "../../index"
import { IFullyManagedTool } from "../ToolExecutorCoordinator"
import { ToolValidator } from "../ToolValidator"
import { TaskConfig } from "../types/TaskConfig"
import { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { BatchProcessor } from "./edit-file/BatchProcessor"
import { EditExecutor } from "./edit-file/EditExecutor"
import { EditFormatter } from "./edit-file/EditFormatter"
import { PreparedFileBatch } from "./edit-file/types"

export class EditFileToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.EDIT_FILE
	private resultsCache = new Map<string, ToolResponse>()
	private lastApiRequestCount = -1

	/** @internal */
	public diagnosticsTimeoutMs = 1000
	/** @internal */
	public diagnosticsDelayMs = 500
	/** @internal - flip-flop for experimental mode */
	public diffMode: "full" | "additions-only" = "full"

	private executor: EditExecutor
	private formatter: EditFormatter
	private processor: BatchProcessor

	constructor(
		private validator: ToolValidator,
		private readonly useLinterOnlyForSyntax: boolean = false,
	) {
		this.executor = new EditExecutor()
		this.formatter = new EditFormatter(this.executor)
		this.processor = new BatchProcessor(
			this.validator,
			this.executor,
			this.formatter,
			this.useLinterOnlyForSyntax,
			this.diagnosticsTimeoutMs,
			this.diagnosticsDelayMs,
			this.diffMode,
		)
	}

	getDescription(block: ToolUse): string {
		const relPath = block.params.path || (Array.isArray(block.params.files) && block.params.files[0]?.path) || ""
		const pathText = relPath ? ` for '${relPath}'` : ""
		return `${block.name}${pathText}`
	}

		async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		let files = block.params.files
		if (typeof files === "string") {
			try { files = JSON.parse(files) } catch (e) {}
		}
		const relPath = Array.isArray(files) && files[0]?.path ? files[0].path : ""
		const filesCount = Array.isArray(files) ? files.length : 0
		const editsCount = Array.isArray(files) ? files.reduce((acc, f) => {
			let edits = f.edits
			if (typeof edits === "string") {
				try { edits = JSON.parse(edits) } catch (e) {}
			}
			return acc + (Array.isArray(edits) ? edits.length : 0)
		}, 0) : 0

		const message = JSON.stringify({
			tool: "editFile",
			path: filesCount > 1 ? "Multiple files" : relPath,
			filesCount,
			editsCount,
		})

		if (await uiHelpers.shouldAutoApproveToolWithPath(this.name, relPath)) {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("ask", "tool")
			await uiHelpers.say("tool", message, undefined, undefined, true)
		} else {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("say", "tool")
			await uiHelpers.ask("tool", message, true).catch(() => {})
		}
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		this.syncCache(config)

		// If we already have a cached result for this block ID, return it immediately.
		if (block.call_id && this.resultsCache.has(block.call_id)) {
			return this.resultsCache.get(block.call_id)!
		}

		// Identify and group all edit_file blocks in the current turn
		const allBatches = this.processor.groupBlocksByPath(config)

		// Check if any block in any batch needs processing
		const needsProcessing = Array.from(allBatches.values()).some((batch: PreparedFileBatch) =>
			batch.blocks.some((b) => !b.call_id || !this.resultsCache.has(b.call_id)),
		)

		if (needsProcessing) {
			const resultsMap = await this.processor.executeMultiFileBatch(config, allBatches)

			// Cache results for all blocks processed in this multi-file batch.
			// Blocks can span multiple files, so we collect and combine results by call_id.
			const combinedResults = new Map<string, ToolResponse[]>()
			for (const batch of allBatches.values()) {
				const result = resultsMap.get(batch.absolutePath) || formatResponse.formatToolErrorForLLM(createToolError("tool.internalError", "Result missing for file.", "recoverable"))
				for (const b of batch.blocks) {
					if (b.call_id) {
						if (!combinedResults.has(b.call_id)) {
							combinedResults.set(b.call_id, [])
						}
						combinedResults.get(b.call_id)!.push(result)
					}
				}
			}

			for (const [callId, fileResults] of combinedResults.entries()) {
				this.resultsCache.set(callId, this.combineResponses(fileResults))
			}
		}

		// Return the result for the current block (either from cache or fallback)
		if (block.call_id && this.resultsCache.has(block.call_id)) {
			return this.resultsCache.get(block.call_id)!
		}

		// Fallback for blocks without call_id or if somehow not in cache
		let files = block.params.files
		let wasStringified = false
		if (typeof files === "string") {
			try { 
				files = JSON.parse(files) 
				block.params.files = files
				wasStringified = true
			} catch (e) {}
		}
		
		let result: ToolResponse
		if (!Array.isArray(files)) {
			const relPath = ""
			const { absolutePath, displayPath } = this.processor.resolvePath(config, relPath)
			const singleBatch = new Map<string, PreparedFileBatch>()
			singleBatch.set(absolutePath, { absolutePath, displayPath, blocks: [block], wasStringified })
			const resultsMap = await this.processor.executeMultiFileBatch(config, singleBatch)
			result = resultsMap.get(absolutePath) || formatResponse.formatToolErrorForLLM(createToolError("tool.internalError", "Unexpected error.", "recoverable"))
		} else {
			const singleBlockBatches = new Map<string, PreparedFileBatch>()
			for (const fe of files) {
				let editsWasStringified = false
				if (typeof fe.edits === "string") {
					try {
						fe.edits = JSON.parse(fe.edits)
						editsWasStringified = true
					} catch (e) {}
				}

				const { absolutePath, displayPath } = this.processor.resolvePath(config, fe.path)
				if (!singleBlockBatches.has(absolutePath)) {
					singleBlockBatches.set(absolutePath, { absolutePath, displayPath, blocks: [], wasStringified: wasStringified || editsWasStringified })
				} else if (wasStringified || editsWasStringified) {
					singleBlockBatches.get(absolutePath)!.wasStringified = true
				}
				singleBlockBatches.get(absolutePath)!.blocks.push({
					...block,
					params: { ...block.params, path: fe.path, edits: fe.edits }
				})
			}
			const resultsMap = await this.processor.executeMultiFileBatch(config, singleBlockBatches)
			result = this.combineResponses(Array.from(resultsMap.values()))
		}

		const isSuccess = typeof result !== "string" || !result.toLowerCase().includes("error")

		const apiConfig = config.services.stateManager.getApiConfiguration()
		const provider = (config.mode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string



		return result
	}

	private combineResponses(responses: ToolResponse[]): ToolResponse {
		if (responses.length === 0) return ""
		if (responses.length === 1) return responses[0]

		if (responses.every((r) => typeof r === "string")) {
			// Deduplicate if they are identical strings (unlikely but possible if same error across files)
			return Array.from(new Set(responses as string[])).join("\n\n")
		}

		const allBlocks: any[] = [] // Using any to avoid complex union types for DiracTextContentBlock | DiracImageContentBlock
		for (const r of responses) {
			if (typeof r === "string") {
				allBlocks.push({ type: "text", text: r })
			} else {
				allBlocks.push(...r)
			}
		}
		return allBlocks
	}

	private syncCache(config: TaskConfig): void {
		// Cache is currently turn-scoped, but we could make it more persistent if needed
		this.resultsCache.clear()
	}
}
