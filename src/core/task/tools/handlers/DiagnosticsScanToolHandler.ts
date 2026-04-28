import * as fs from "fs/promises"
import { getReadablePath, isLocatedInWorkspace } from "@/utils/path"
import { arePathsEqual } from "@/utils/path"
import { ToolUse } from "@core/assistant-message"
import { HostProvider } from "@/hosts/host-provider"
import { resolveWorkspacePath } from "@core/workspace"
import { Diagnostic, DiagnosticSeverity, FileDiagnostics } from "@/shared/proto/index.dirac"
import { DiracDefaultTool } from "@/shared/tools"
import { telemetryService } from "@/services/telemetry"

import { ToolResponse } from "../../index"
import { IFullyManagedTool } from "../ToolExecutorCoordinator"
import { ToolValidator } from "../ToolValidator"
import { TaskConfig } from "../types/TaskConfig"
import { StronglyTypedUIHelpers } from "../types/UIHelpers"

export class DiagnosticsScanToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.DIAGNOSTICS_SCAN

	public baseDiagnosticsTimeoutMs = 2000
	public diagnosticsDelayMs = 500

	constructor(private validator: ToolValidator) {}

	getDescription(block: ToolUse): string {
		const relPaths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : [])
		const pathsText = relPaths.length > 0 ? ` for ${relPaths.map((p) => `'${p}'`).join(", ")}` : ""
		return `[${block.name}${pathsText}]`
	}

	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		const relPaths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : [])
		const config = uiHelpers.getConfig()

		const message = JSON.stringify({
			tool: "diagnosticsScan",
			paths: relPaths.map((p) => getReadablePath(config.cwd, uiHelpers.removeClosingTag(block, "paths", p))),
			operationIsLocatedInWorkspace: (await Promise.all(relPaths.map((p) => isLocatedInWorkspace(p)))).every(Boolean),
		})

		const firstPath = relPaths[0] || ""
		if (await uiHelpers.shouldAutoApproveToolWithPath(block.name, firstPath)) {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("ask", "tool")
			await uiHelpers.say("tool", message, undefined, undefined, true)
		} else {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("say", "tool")
			await uiHelpers.ask("tool", message, true).catch(() => {})
		}
	}

		async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const validation = this.validator.assertRequiredParams(block, "paths")
		if (!validation.ok) {
			config.taskState.consecutiveMistakeCount++
			return await config.callbacks.sayAndCreateMissingParamError(this.name, "paths")
		}

		const relPaths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : [])
		if (relPaths.length === 0) {
			config.taskState.consecutiveMistakeCount++
			return await config.callbacks.sayAndCreateMissingParamError(this.name, "paths")
		}

		const fileInfos = await Promise.all(
			relPaths.map(async (relPath) => {
				const pathResult = resolveWorkspacePath(config, relPath, "DiagnosticsScanToolHandler.execute")
				const { absolutePath, displayPath } =
					typeof pathResult === "string" ? { absolutePath: pathResult, displayPath: relPath } : pathResult

				try {
					const content = await fs.readFile(absolutePath, "utf8")
					return { absolutePath, displayPath, content, error: undefined }
				} catch (error) {
					return { absolutePath, displayPath, content: "", error: error instanceof Error ? error.message : String(error) }
				}
			})
		)

		const errorResults = fileInfos.filter((f) => f.error).map((f) => `- file: ${f.displayPath}\n  error: ${f.error}`)
		const validFiles = fileInfos.filter((f) => !f.error)

		if (validFiles.length === 0) {
			return errorResults.join("\n---\n")
		}

		// Send an intermediate partial message to show that we are now fetching diagnostics
		const intermediateMessage = JSON.stringify({
			tool: "diagnosticsScan",
			paths: relPaths.map((p) => getReadablePath(config.cwd, p)),
			operationIsLocatedInWorkspace: (await Promise.all(relPaths.map((p) => isLocatedInWorkspace(p)))).every(Boolean),
		})
		await config.callbacks.say("tool", intermediateMessage, undefined, undefined, true)

		// Prepare diagnostics for each file (e.g. by loading them into memory)
		await HostProvider.workspace.prepareDiagnostics({
			filePaths: validFiles.map((f) => f.absolutePath),
		})

		// Polling logic to wait for diagnostics to be available
		const totalLines = validFiles.reduce((sum, f) => sum + f.content.split(/\r?\n/).length, 0)
		const timeoutMs = Math.min(this.baseDiagnosticsTimeoutMs + Math.floor(totalLines / 1000) * 1000, 10000)
		const startTime = Date.now()
		let allDiagnostics: FileDiagnostics[] = []
		let foundDiagnostics = false

		while (Date.now() - startTime < timeoutMs) {
			const response = await HostProvider.workspace.getDiagnostics({
				filePaths: validFiles.map((f) => f.absolutePath),
			})
			allDiagnostics = response.fileDiagnostics || []

			// Check if we found any diagnostics for the requested files
			foundDiagnostics = validFiles.some((f) => {
				const fileDiags = allDiagnostics.find((d: any) => arePathsEqual(d.filePath, f.displayPath) || arePathsEqual(d.filePath, f.absolutePath))
				return (
					fileDiags?.diagnostics.some(
						(d: any) => d.severity === DiagnosticSeverity.DIAGNOSTIC_ERROR || d.severity === DiagnosticSeverity.DIAGNOSTIC_WARNING
					) ?? false
				)
			})

			if (foundDiagnostics) {
				break
			}

			// Wait before next poll
			await new Promise((resolve) => setTimeout(resolve, this.diagnosticsDelayMs))
		}

		const results = validFiles.map((f) => {
			const fileDiags = allDiagnostics.find((d: any) => arePathsEqual(d.filePath, f.displayPath) || arePathsEqual(d.filePath, f.absolutePath))
			const allProblems =
				fileDiags?.diagnostics.filter(
					(d: any) => d.severity === DiagnosticSeverity.DIAGNOSTIC_ERROR || d.severity === DiagnosticSeverity.DIAGNOSTIC_WARNING
				) || []

			if (allProblems.length === 0) {
				return `- file: ${f.displayPath}\n  status: No diagnostics issues found.`
			}

			const maxErrors = 20
			const errCtxLines = 1
			const problems = allProblems.slice(0, maxErrors)
			const truncatedCount = allProblems.length - problems.length

			// Group diagnostics by line to avoid duplicate context blocks
			const diagnosticsByLine = new Map<number, Diagnostic[]>()
			for (const d of problems) {
				const line = d.range?.start?.line ?? -1
				if (!diagnosticsByLine.has(line)) {
					diagnosticsByLine.set(line, [])
				}
				diagnosticsByLine.get(line)!.push(d)
			}

			const lines = f.content.split(/\r?\n/)
			const formattedProblems = Array.from(diagnosticsByLine.entries())
				.sort(([lineA], [lineB]) => lineA - lineB)
				.map(([lineIdx, diags]) => {
					const lineNum = lineIdx + 1
					const contextStart = Math.max(0, lineIdx - errCtxLines)
					const contextEnd = Math.min(lines.length - 1, lineIdx + errCtxLines)

					const context = lines
						.slice(contextStart, contextEnd + 1)
						.map((l, i) => {
							const currentLineIdx = contextStart + i
							const isTargetLine = currentLineIdx === lineIdx
							let lineText = l
							if (isTargetLine) {
								const messages = diags
									.map((d) => {
										const label = d.severity === DiagnosticSeverity.DIAGNOSTIC_ERROR ? "Error" : "Warning"
										return `[${label}] Line ${lineNum}: ${d.message}`
									})
									.join("\n    ")
								lineText = `${l} <<<< ${messages}`
							}
							return `    ${lineText}`
						})
						.join("\n")

					return context
				})
				.join("\n\n")

			const truncationNote = truncatedCount > 0 ? `\n\n    ... and ${truncatedCount} more errors.` : ""
			return `- file: ${f.displayPath}\n  diagnostics: |\n${formattedProblems}${truncationNote}`
		})

		const finalResult = [...errorResults, ...results].join("\n---\n")

		const finalMessage = JSON.stringify({
			tool: "diagnosticsScan",
			paths: relPaths.map((p) => getReadablePath(config.cwd, p)),
			content: finalResult,
			operationIsLocatedInWorkspace: (await Promise.all(relPaths.map((p) => isLocatedInWorkspace(p)))).every(Boolean),
		})

		await config.callbacks.removeLastPartialMessageIfExistsWithType("say", "tool")
		await config.callbacks.say("tool", finalMessage, undefined, undefined, false)

		const apiConfig = config.services.stateManager.getApiConfiguration()
		const provider = (config.mode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string

		telemetryService.captureToolUsage(
			config.ulid,
			this.name,
			config.api.getModel().id,
			provider,
			false, // autoApproved - diagnostics_scan is never auto-approved in the current implementation
			true, // success - if we reached here, it's a success
			undefined,
			block.isNativeToolCall,
		)


		return finalResult
	}

}
