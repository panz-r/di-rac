import { setTimeout as setTimeoutPromise } from "node:timers/promises"
import { ToolUse } from "@core/assistant-message"
import { formatResponse } from "@core/prompts/responses"
import { resolveWorkspacePath } from "@core/workspace"
import { FileAnchorIndex } from "@shared/utils/file-anchor-index"
import { isLocatedInWorkspace } from "@utils/path"
import * as fs from "fs/promises"
import * as path from "path"
import { HostProvider } from "@/hosts/host-provider"
import { getDiagnosticsProviders } from "@/integrations/diagnostics/getDiagnosticsProviders"

import { DiracSayTool } from "@/shared/ExtensionMessage"
import { DiracDefaultTool } from "@/shared/tools"
import { ToolResponse } from "../../../index"
import { createToolError } from "@shared/tool-response"
import { showNotificationForApproval } from "../../../utils"
import { ToolValidator } from "../../ToolValidator"
import { TaskConfig } from "../../types/TaskConfig"
import { ToolResultUtils } from "../../utils/ToolResultUtils"
import { EditExecutor } from "./EditExecutor"
import { EditFormatter } from "./EditFormatter"
import { FileEdit, PreparedEdits, PreparedFileBatch } from "./types"

export class BatchProcessor {
    constructor(
        private validator: ToolValidator,
        private executor: EditExecutor,
        private formatter: EditFormatter,
        private useLinterOnlyForSyntax: boolean,
        private diagnosticsTimeoutMs: number,
        private diagnosticsDelayMs: number,
        private diffMode: "full" | "additions-only",
    ) { }

    resolvePath(config: TaskConfig, relPath: string): { absolutePath: string; displayPath: string } {
        const pathResult = resolveWorkspacePath(config, relPath, "EditFileToolHandler.resolvePath")
        return typeof pathResult === "string" ? { absolutePath: pathResult, displayPath: relPath } : pathResult
    }

    groupBlocksByPath(config: TaskConfig): Map<string, PreparedFileBatch> {
        const allBlocks = config.taskState.assistantMessageContent.filter(
            (b: any): b is ToolUse => b.type === "tool_use" && b.name === DiracDefaultTool.EDIT_FILE,
        )

        const groups = new Map<string, PreparedFileBatch>()

        for (const b of allBlocks) {
            const fileEdits: FileEdit[] = []
            
            let files = b.params.files
            let wasStringified = false
            if (typeof files === "string") {
                try {
                    files = JSON.parse(files)
                    b.params.files = files
                    wasStringified = true
                } catch (e) { }
            }

            if (Array.isArray(files)) {
                fileEdits.push(...files)
            }

            for (const fe of fileEdits) {
                let editsWasStringified = false
                if (typeof fe.edits === "string") {
                    try {
                        fe.edits = JSON.parse(fe.edits)
                        editsWasStringified = true
                    } catch (e) { }
                }

                const { absolutePath, displayPath } = this.resolvePath(config, fe.path)
                if (!groups.has(absolutePath)) {
                    groups.set(absolutePath, { absolutePath, displayPath, blocks: [], wasStringified: wasStringified || editsWasStringified })
                } else if (wasStringified || editsWasStringified) {
                    groups.get(absolutePath)!.wasStringified = true
                }

                groups.get(absolutePath)!.blocks.push({
                    ...b,
                    params: { ...b.params, path: fe.path, edits: fe.edits },
                } as any)
            }
        }
        return groups
    }

    async executeMultiFileBatch(
        config: TaskConfig,
        allBatches: Map<string, PreparedFileBatch>,
    ): Promise<Map<string, ToolResponse>> {
        const results = new Map<string, ToolResponse>()
        const preparedBatches: PreparedFileBatch[] = []

        for (const batch of allBatches.values()) {
            const { error, prepared } = await this.validateAndPrepare(config, batch.absolutePath, batch.displayPath, batch.blocks)
            if (error) {
                results.set(batch.absolutePath, error)
            } else if (prepared) {
                // Apply all edits in memory for this batch
                const { finalLines, appliedEdits } = this.executor.applyEdits(prepared.lines, prepared.resolvedEdits)
                prepared.finalLines = finalLines
                prepared.finalContent = finalLines.join("\n")
                prepared.appliedEdits = appliedEdits

                // Generate diff for the summary
                let diff = `*** Update File: ${batch.displayPath}\n\n`
                for (const applied of appliedEdits) {
                    const editType = applied.edit.edit_type
                    let searchContent: string
                    let replaceContent: string

                    if (editType === "insert_after") {
                        searchContent = prepared.lines[applied.originalStartIdx]
                        replaceContent = `${searchContent}\n${applied.edit.text}`
                    } else if (editType === "insert_before") {
                        searchContent = prepared.lines[applied.originalStartIdx]
                        replaceContent = `${applied.edit.text}\n${searchContent}`
                    } else {
                        searchContent = prepared.lines.slice(applied.originalStartIdx, applied.originalEndIdx + 1).join("\n")
                        replaceContent = applied.edit.text
                    }

                    diff += `<<<<<<< SEARCH\n${searchContent}\n=======\n${replaceContent}\n>>>>>>> REPLACE\n\n`
                }
                prepared.diff = diff

                preparedBatches.push({ ...batch, prepared })
            }
        }

        if (preparedBatches.length === 0) {
            return results
        }

        await config.callbacks.removeLastPartialMessageIfExistsWithType("say", "tool")
        await config.callbacks.removeLastPartialMessageIfExistsWithType("ask", "tool")

        // Send an intermediate partial message to show that we are now applying edits and running diagnostics
        const intermediateMessage = await this.buildEditMessage(config, preparedBatches)
        await config.callbacks.say("tool", JSON.stringify(intermediateMessage), undefined, undefined, true)


        const shouldAutoApprove = await this.checkAutoApproval(config, preparedBatches)
        if (!shouldAutoApprove) {
            const didApprove = await this.requestCombinedApproval(config, preparedBatches)
            if (!didApprove) {
                const denied = formatResponse.toolDenied()
                for (const batch of preparedBatches) {
                    results.set(batch.absolutePath, denied)
                }
                return results
            }
        } else {
            const completeMessage = await this.buildEditMessage(config, preparedBatches)
            await config.callbacks.say("tool", JSON.stringify(completeMessage), undefined, undefined, true)
        }

        const providers = getDiagnosticsProviders(
            this.useLinterOnlyForSyntax,
            this.diagnosticsTimeoutMs,
            this.diagnosticsDelayMs,
        )

        // 1. Capture pre-save diagnostics for all files once
        const preDiagnostics = (
            await Promise.all(providers.map((p) => p.capturePreSaveState()))
        ).flat()

        const appliedResults = new Map<string, {
            saveResult: { finalContent: string; autoFormattingEdits?: string; userEdits?: string }
            finalContent: string
            finalLines: string[]
            newLineHashes: string[]
        }>()

        let anyFailed = false;
        let anySucceeded = false;
        // 2. Apply and save all files (sequentially to avoid UI mess)
        for (let i = 0; i < preparedBatches.length; i++) {
            const batch = preparedBatches[i]
            const isLast = i === preparedBatches.length - 1
            try {
                const applied = await this.applyAndSave(config, batch, { silent: !isLast })
                appliedResults.set(batch.absolutePath, applied)
                anySucceeded = true;
            } catch (error) {
                anyFailed = true;
                const errorMessage = error instanceof Error ? error.message : String(error)
                results.set(batch.absolutePath, formatResponse.formatToolErrorForLLM(createToolError("io.file.locked", `Error applying edits to ${batch.displayPath}: ${errorMessage}`, "recoverable")))
            } finally {
                await config.services.diffViewProvider.reset().catch(() => { })
            }

        }

        if (anyFailed) {
            config.taskState.consecutiveMistakeCount++;
        } else if (anySucceeded) {
            config.taskState.consecutiveMistakeCount = 0;
        }

        // 3. Run diagnostics for all successfully applied files in parallel
        const successfulBatches = preparedBatches.filter((b) => appliedResults.has(b.absolutePath))
        if (successfulBatches.length > 0) {
            const diagnosticsData = successfulBatches.map((b) => {
                const applied = appliedResults.get(b.absolutePath)!
                return {
                    filePath: b.absolutePath,
                    content: applied.finalContent,
                    hashes: applied.newLineHashes,
                }
            })

            const providerDiagnostics = await Promise.all(
                providers.map((p) => p.getDiagnosticsFeedbackForFiles(diagnosticsData, preDiagnostics))
            )

            // Combine diagnostics from all providers for each file
            for (let i = 0; i < successfulBatches.length; i++) {
                const batch = successfulBatches[i]
                const applied = appliedResults.get(batch.absolutePath)!
                let finalDiagnosticsResult = { newProblemsMessage: "", fixedCount: 0 }

                for (const resultsOfProvider of providerDiagnostics) {
                    const res = resultsOfProvider[i]
                    if (res.newProblemsMessage && !finalDiagnosticsResult.newProblemsMessage) {
                        // Found new problems, stop and return these (prioritize first provider with problems)
                        finalDiagnosticsResult.newProblemsMessage = res.newProblemsMessage
                    }
                    finalDiagnosticsResult.fixedCount += res.fixedCount
                }

                batch.diagnostics = finalDiagnosticsResult

                // 4. Format final results
                const result = this.formatter.createResultsResponse(
                    batch.prepared!,
                    applied.finalLines,
                    applied.newLineHashes,
                    finalDiagnosticsResult,
                    this.diffMode,
                    applied.saveResult.autoFormattingEdits,
                    applied.saveResult.userEdits,
                    batch.wasStringified
                )
                results.set(batch.absolutePath, result)
            }
        }


        await config.callbacks.removeLastPartialMessageIfExistsWithType("say", "tool")
        await config.callbacks.removeLastPartialMessageIfExistsWithType("ask", "tool")

        const finalMessage = await this.buildEditMessage(config, preparedBatches)
        await config.callbacks.say("tool", JSON.stringify(finalMessage), undefined, undefined, false)

        return results
    }

    async validateAndPrepare(
        config: TaskConfig,
        absolutePath: string,
        displayPath: string,
        blocks: ToolUse[],
    ): Promise<{ error?: ToolResponse; prepared?: PreparedEdits }> {
        for (const block of blocks) {
            if (block.params.path === undefined || block.params.edits === undefined) {
                let files = block.params.files
                if (typeof files === "string") {
                    try { files = JSON.parse(files) } catch (e) { }
                }
                if (!Array.isArray(files)) {
                    config.taskState.consecutiveMistakeCount++
                    return { error: formatResponse.formatToolErrorForLLM(createToolError("tool.unknownError", "Invalid edit_file arguments. Use: edit_file <path> --anchor <id> --content <text> [--end-anchor <id>] [--edit-type TYPE]", "recoverable")) }
                }
            }

            const edits = block.params.edits
            if (!Array.isArray(edits)) {
                config.taskState.consecutiveMistakeCount++
                return { error: formatResponse.formatToolErrorForLLM(createToolError("tool.unknownError", "Invalid edit_file arguments. Each edit requires --anchor and --content.", "recoverable")) }
            }

            for (const edit of edits) {
                const editType = edit.edit_type
                const hasEndAnchor = !!edit.end_anchor
                const isReplace = editType === "replace" || !editType // default is replace

                if (!editType || !edit.anchor || (isReplace && !hasEndAnchor) || edit.text === undefined) {
                    config.taskState.consecutiveMistakeCount++
                    const missingField = !editType
                        ? "edit_type"
                        : !edit.anchor
                            ? "anchor"
                            : isReplace && !hasEndAnchor
                                ? "end_anchor"
                                : "text"
                    return { error: formatResponse.formatToolErrorForLLM(createToolError("tool.unknownError", `Each edit must contain '${missingField}'.`, "recoverable")) }
                }
            }
        }

        const preparedResult = await this.prepareEdits(config, absolutePath, displayPath, blocks)
        if ("error" in preparedResult) {
            return { error: preparedResult.error }
        }

        return { prepared: preparedResult }
    }

    async checkAutoApproval(config: TaskConfig, batches: PreparedFileBatch[]): Promise<boolean> {
        if (config.isSubagentExecution) return true
        for (const batch of batches) {
            const allowed = await config.callbacks.shouldAutoApproveToolWithPath(DiracDefaultTool.EDIT_FILE, batch.displayPath)
            if (!allowed) return false
        }
        return true
    }

    async requestCombinedApproval(config: TaskConfig, batches: PreparedFileBatch[]): Promise<boolean> {
        const totalRequestedEdits = batches.reduce(
            (acc, b) =>
                acc + b.blocks.reduce((acc2, b2) => acc2 + (Array.isArray(b2.params.edits) ? b2.params.edits.length : 0), 0),
            0,
        )

        const fileNames = batches.map((b) => path.basename(b.absolutePath)).join(", ")
        const notificationMessage = `Dirac wants to edit ${fileNames} with ${totalRequestedEdits} anchored edits`
        showNotificationForApproval(notificationMessage, config.autoApprovalSettings.enableNotifications)

        const completeMessage = await this.buildEditMessage(config, batches)
        await config.callbacks.removeLastPartialMessageIfExistsWithType("say", "tool")
        await config.callbacks.removeLastPartialMessageIfExistsWithType("ask", "tool")

        const { didApprove } = await ToolResultUtils.askApprovalAndPushFeedback("tool", JSON.stringify(completeMessage), config)
        return didApprove
    }

    async buildEditMessage(config: TaskConfig, batches: PreparedFileBatch[]): Promise<DiracSayTool> {
        const totalRequestedEdits = batches.reduce(
            (acc, b) =>
                acc + b.blocks.reduce((acc2, b2) => acc2 + (Array.isArray(b2.params.edits) ? b2.params.edits.length : 0), 0),
            0,
        )

        const warningCount = batches.reduce((acc, b) => acc + (b.prepared?.failedEdits.length || 0), 0)
        const warning = warningCount > 0 ? `\n\nWarning: ${warningCount} edit(s) failed to resolve and will be skipped.` : ""
        const diffs = batches.map((b) => b.prepared?.diff).join("\n\n")

        const editSummaries = await Promise.all(
            batches.map(async (b) => ({
                path: b.displayPath,
                edits:
                    b.prepared?.appliedEdits.map((ae) => ({
                        additions: ae.linesAdded,
                        deletions: ae.linesDeleted,
                    })) || [],
                diagnostics: b.diagnostics,
            })),
        )

        const operationIsLocatedInWorkspace =
            batches.length === 1
                ? await isLocatedInWorkspace(batches[0].absolutePath)
                : (await Promise.all(batches.map((b) => isLocatedInWorkspace(b.absolutePath)))).every(Boolean)

        return {
            tool: "editFile",
            path: batches.length === 1 ? batches[0].displayPath : "Multiple files",
            filesCount: batches.length,
            editsCount: totalRequestedEdits,
            diff: diffs + warning,
            editSummaries,
            operationIsLocatedInWorkspace,
        }
    }

    async applyAndSave(
        config: TaskConfig,
        batch: PreparedFileBatch,
        options: { silent: boolean },
    ): Promise<{
        saveResult: { finalContent: string; autoFormattingEdits?: string; userEdits?: string }
        finalContent: string
        finalLines: string[]
        newLineHashes: string[]
    }> {
        const { absolutePath, displayPath, prepared } = batch
        if (!prepared) throw new Error("Failed to prepare edits.")

        let { finalContent, finalLines } = prepared

        if (options.silent) {
            const saveResult = await config.services.diffViewProvider.applyAndSaveSilently(absolutePath, finalContent)
            const actualFinalContent = saveResult.finalContent || finalContent

            if (actualFinalContent !== finalContent) {
                finalContent = actualFinalContent
                finalLines = finalContent.split(/\r?\n/)
            }

            config.taskState.consecutiveMistakeCount = 0
            config.taskState.didEditFile = true
            config.services.fileContextTracker.markFileAsEditedByDirac(displayPath)
            await config.services.fileContextTracker.trackFileContext(displayPath, "dirac_edited")

            const newLineHashes = new FileAnchorIndex(finalLines).getAllHashes()

            return {
                saveResult: {
                    finalContent: actualFinalContent,
                    autoFormattingEdits: saveResult.autoFormattingEdits,
                    userEdits: saveResult.userEdits,
                },
                finalContent,
                finalLines,
                newLineHashes,
            }
        }


        config.services.diffViewProvider.editType = "modify"
        // Stage the changes in the diff view provider before saving
        await config.services.diffViewProvider.open(absolutePath, { displayPath })
        await config.services.diffViewProvider.update(finalContent, true)

        // Wait for the diff view to update before saving to ensure auto-formatting is triggered
        await setTimeoutPromise(200)

        // Save the changes and get the final content (including any auto-formatting)
        // Save the changes and get the final content (including any auto-formatting)
        // We skip diagnostics here because executeMultiFileBatch handles them in parallel for the whole batch
        const saveResult = await this.saveAndTrackChanges(config, absolutePath, displayPath, finalContent, { skipDiagnostics: true })

        // Update finalContent and finalLines if they changed during save (e.g. auto-formatting)
        if (saveResult.finalContent !== finalContent) {
            finalContent = saveResult.finalContent
            finalLines = finalContent.split(/\r?\n/)
        }

        const newLineHashes = new FileAnchorIndex(finalLines).getAllHashes()

        return { saveResult, finalContent, finalLines, newLineHashes }
    }

    async prepareEdits(
        config: TaskConfig,
        absolutePath: string,
        displayPath: string,
        blocks: ToolUse[],
    ): Promise<PreparedEdits | { error: ToolResponse }> {
        try {
            await HostProvider.workspace.saveOpenDocumentIfDirty({ filePath: absolutePath })
            const content = await fs.readFile(absolutePath, "utf8")
            const lines = content.split(/\r?\n/)
            const index = new FileAnchorIndex(lines)
            const lineHashes = index.getAllHashes()

            const { resolvedEdits, failedEdits } = this.executor.resolveEdits(blocks, lines, lineHashes, index, absolutePath)

            if (resolvedEdits.length === 0) {
                const failureMessages = failedEdits.map((f) => this.executor.formatFailureMessage(f.edit, f.error))
                return { error: formatResponse.formatToolErrorForLLM(createToolError("anchor.notFound", failureMessages.join("\n\n"), "recoverable")) }
            }

            return {
                content,
                finalContent: content, // Placeholder
                diff: "", // Placeholder
                resolvedEdits,
                failedEdits,
                appliedEdits: [], // Placeholder
                lines,
                lineHashes,
                finalLines: lines, // Placeholder
            }
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : String(error)
            return { error: formatResponse.formatToolErrorForLLM(createToolError("io.file.notFound", `Error preparing edits: ${errorMessage}`, "recoverable")) }
        }
    }

    async saveAndTrackChanges(
        config: TaskConfig,
        absolutePath: string,
        displayPath: string,
        finalContent: string,
        options?: { skipDiagnostics?: boolean },
    ): Promise<{ finalContent: string; autoFormattingEdits?: string; userEdits?: string }> {
        // Use DiffViewProvider to save changes, which handles auto-formatting and VS Code document synchronization
        const saveResult = await config.services.diffViewProvider.saveChanges(options)
        const actualFinalContent = saveResult.finalContent || finalContent

        config.taskState.consecutiveMistakeCount = 0
        config.taskState.didEditFile = true
        config.services.fileContextTracker.markFileAsEditedByDirac(displayPath)
        await config.services.fileContextTracker.trackFileContext(displayPath, "dirac_edited")

        return {
            finalContent: actualFinalContent,
            autoFormattingEdits: saveResult.autoFormattingEdits,
            userEdits: saveResult.userEdits,
        }
    }
}
