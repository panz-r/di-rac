// tool call test comment
import type { ToolUse } from "@core/assistant-message"
import { regexSearchFiles } from "@services/ripgrep"
import { DiracSayTool } from "@shared/ExtensionMessage"
import { stripHashes } from "@utils/line-hashing"
import { arePathsEqual, getReadablePath, isLocatedInWorkspace } from "@utils/path"
import { createToolError } from "@shared/tool-response"
import * as path from "path"
import { formatResponse } from "@/core/prompts/responses"
import { parseWorkspaceInlinePath } from "@/core/workspace/utils/parseWorkspaceInlinePath"
import { WorkspacePathAdapter } from "@/core/workspace/WorkspacePathAdapter"
import { resolveWorkspacePath } from "@/core/workspace/WorkspaceResolver"
import { Logger } from "@/shared/services/Logger"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import { showNotificationForApproval } from "../../utils"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { ToolResultUtils } from "../utils/ToolResultUtils"

export class SearchFilesToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.SEARCH

	constructor(private validator: ToolValidator) {}
	private getRelPaths(params: any): string[] {
		return Array.isArray(params.paths)
			? params.paths
			: params.paths
				? [params.paths as string]
				: Array.isArray(params.path)
					? params.path
					: params.path
						? [params.path as string]
						: []
	}

	getDescription(block: ToolUse): string {
		const relPaths = this.getRelPaths(block.params)
		const pathsStr = relPaths.length > 1 ? `${relPaths.length} paths` : `'${relPaths[0] || ""}'`
		const contextLines = block.params.context_lines ? ` with ${block.params.context_lines} context lines` : ""
		return `${block.name} '${block.params.regex}' in ${pathsStr}${
			block.params.file_pattern ? ` matching '${block.params.file_pattern}'` : ""
		}${contextLines}`
	}

	/**
	 * Determines which paths to search based on workspace configuration and hints
	 */
	private determineSearchPaths(
		config: TaskConfig,
		parsedPath: string,
		workspaceHint: string | undefined,
		originalPath: string,
	): Array<{ absolutePath: string; workspaceName?: string; workspaceRoot?: string }> {
		if (config.isMultiRootEnabled && config.workspaceManager) {
			const adapter = new WorkspacePathAdapter({
				cwd: config.cwd,
				isMultiRootEnabled: true,
				workspaceManager: config.workspaceManager,
			})

			if (workspaceHint) {
				// Search only in the specified workspace
				const absolutePath = adapter.resolvePath(parsedPath, workspaceHint)
				const workspaceRoots = adapter.getWorkspaceRoots()
				const root = workspaceRoots.find((r) => r.name === workspaceHint)
				return [{ absolutePath, workspaceName: workspaceHint, workspaceRoot: root?.path }]
			}
			// As a fallback, perform the search across all available workspaces.
			// Typically, models should provide explicit hints to target specific workspaces for searching.
			const allPaths = adapter.getAllPossiblePaths(parsedPath)
			const workspaceRoots = adapter.getWorkspaceRoots()
			return allPaths.map((absPath, index) => ({
				absolutePath: absPath,
				workspaceName: workspaceRoots[index]?.name || path.basename(workspaceRoots[index]?.path || absPath),
				workspaceRoot: workspaceRoots[index]?.path,
			}))
		}
		// Single-workspace mode (backward compatible)
		const pathResult = resolveWorkspacePath(config, originalPath, "SearchFilesTool.execute")
		const absolutePath = typeof pathResult === "string" ? pathResult : pathResult.absolutePath
		return [{ absolutePath, workspaceRoot: config.cwd }]
	}

	/**
	 * Executes a single search operation in a workspace
	 */
	private async executeSearch(
		config: TaskConfig,
		absolutePath: string,
		workspaceName: string | undefined,
		workspaceRoot: string | undefined,
		regex: string,
		filePattern: string | undefined,
		contextLines: number | undefined,
		excludeFilePatterns?: string[],
	) {
		try {
			// Use workspace root for relative path calculation, fallback to cwd
			const basePathForRelative = workspaceRoot || config.cwd

			const workspaceResults = await regexSearchFiles(
				basePathForRelative,
				absolutePath,
				regex,
				filePattern,
				config.services.diracIgnoreController,
				config.ulid,
				contextLines,
				excludeFilePatterns,
			)

			// Parse the result count from the first line
			const firstLine = workspaceResults.split("\n")[0]
			const resultMatch = firstLine.match(/Found (\d+) result/)
			const resultCount = resultMatch ? Number.parseInt(resultMatch[1], 10) : 0

			return {
				workspaceName,
				workspaceResults,
				resultCount,
				success: true,
			}
		} catch (error) {
			// If search fails in one workspace, return error info
			Logger.error(`Search failed in ${absolutePath}:`, error)
			return {
				workspaceName,
				workspaceResults: "",
				resultCount: 0,
				success: false,
			}
		}
	}

	/**
	 * Formats search results based on workspace configuration
	 */
	private formatSearchResults(
		config: TaskConfig,
		searchResults: Array<{
			workspaceName?: string
			workspaceResults: string
			resultCount: number
			success: boolean
		}>,
		searchPaths: Array<{ absolutePath: string; workspaceName?: string }>,
	): string {
		const allResults: string[] = []
		let totalResultCount = 0

		for (const { workspaceName, workspaceResults, resultCount, success } of searchResults) {
			if (!success || !workspaceResults) {
				continue
			}

			totalResultCount += resultCount

			// If multi-workspace and we have results, annotate with workspace name
			if (config.isMultiRootEnabled && searchPaths.length > 1 && workspaceName) {
				// Check if this workspace has results (resultCount > 0)
				if (resultCount > 0) {
					// Skip the "Found X results" line and add workspace annotation
					const lines = workspaceResults.split("\n")
					// Skip first two lines (count and empty line) if they exist
					const resultsWithoutHeader = lines.length > 2 ? lines.slice(2).join("\n") : workspaceResults

					if (resultsWithoutHeader.trim()) {
						allResults.push(`## Workspace: ${workspaceName}\n${resultsWithoutHeader}`)
					}
				}
				// Don't add anything for workspaces with 0 results in multi-workspace mode
			} else if (!config.isMultiRootEnabled || searchPaths.length === 1) {
				// Single workspace mode or single workspace search
				allResults.push(workspaceResults)
			}
		}

		// Combine results
		if (config.isMultiRootEnabled && searchPaths.length > 1) {
			// Multi-workspace search result
			if (totalResultCount === 0) {
				return "Found 0 results."
			}
			return `Found ${totalResultCount === 1 ? "1 result" : `${totalResultCount.toLocaleString()} results`} across ${searchPaths.length} workspace${searchPaths.length > 1 ? "s" : ""}.\n\n${allResults.join("\n\n")}`
		}
		// Single workspace result
		return allResults[0] || "Found 0 results."
	}

	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		const relPaths = this.getRelPaths(block.params)
		const relPath = relPaths[0] || ""
		const regex = block.params.regex

		const config = uiHelpers.getConfig()
		if (config.isSubagentExecution) {
			return
		}

		// Create and show partial UI message
		const filePattern = block.params.file_pattern
		const contextLines = block.params.context_lines

		const sharedMessageProps = {
			tool: "searchFiles",
			paths: relPaths.map((p) =>
				getReadablePath(config.cwd, uiHelpers.removeClosingTag(block, block.params.paths ? "paths" : "path", p)),
			),
			path: getReadablePath(config.cwd, uiHelpers.removeClosingTag(block, block.params.paths ? "paths" : "path", relPath)),
			content: "",
			regex: uiHelpers.removeClosingTag(block, "regex", regex),
			filePattern: uiHelpers.removeClosingTag(block, "file_pattern", filePattern),
			contextLines: Number.parseInt(uiHelpers.removeClosingTag(block, "context_lines", contextLines) || "0", 10),
			operationIsLocatedInWorkspace: (await Promise.all(relPaths.map((p) => isLocatedInWorkspace(p)))).every(Boolean),
		} satisfies DiracSayTool

		const partialMessage = JSON.stringify(sharedMessageProps)

		// Handle auto-approval vs manual approval for partial
		const shouldAutoApprove = (
			await Promise.all(relPaths.map((p) => uiHelpers.shouldAutoApproveToolWithPath(block.name, p)))
		).every(Boolean)

		if (shouldAutoApprove) {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("ask", "tool")
			await uiHelpers.say("tool", partialMessage, undefined, undefined, block.partial)
		} else {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("say", "tool")
			await uiHelpers.ask("tool", partialMessage, block.partial).catch(() => {})
		}
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const relPaths = this.getRelPaths(block.params)
		const regex: string | undefined = block.params.regex
		const filePattern: string | undefined = block.params.file_pattern
		const contextLines = Number.parseInt(block.params.context_lines || "0", 10)

		// Extract provider information for telemetry
		const apiConfig = config.services.stateManager.getApiConfiguration()
		const currentMode = config.services.stateManager.getGlobalSettingsKey("mode")
		const provider = (currentMode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string

		// Validate required parameters
		if (relPaths.length === 0) {
			config.taskState.consecutiveMistakeCount++
			return await config.callbacks.sayAndCreateMissingParamError(this.name, block.params.paths ? "paths" : "path")
		}

		if (!regex) {
			config.taskState.consecutiveMistakeCount++
			return await config.callbacks.sayAndCreateMissingParamError(this.name, "regex")
		}

		// Parse workspace hint from the path and determine search targets.
		// These can throw if the workspace configuration is invalid or the
		// path cannot be resolved, so catch and return a graceful tool error.
		let anyUsedWorkspaceHint = false
		const allSearchPaths: Array<{ absolutePath: string; workspaceName?: string; workspaceRoot?: string; originalPath: string }> = []

		try {
			for (const relPath of relPaths) {
				const parsed = parseWorkspaceInlinePath(relPath)
				const searchPaths = this.determineSearchPaths(config, parsed.relPath, parsed.workspaceHint, relPath)
				allSearchPaths.push(...searchPaths.map((p) => ({ ...p, originalPath: relPath })))
				if (parsed.workspaceHint) {
					anyUsedWorkspaceHint = true
				}
			}
		} catch (error) {
			config.taskState.consecutiveMistakeCount++
			const errorMessage = error instanceof Error ? error.message : String(error)
			return formatResponse.formatToolErrorForLLM(createToolError("tool.internalError", `Error resolving search path: ${errorMessage}`, "recoverable"))
		}

		// Determine workspace context for telemetry
		const primaryWorkspaceRoot = allSearchPaths[0]?.workspaceRoot
		const resolvedToNonPrimary =
			allSearchPaths.length === 0
				? true
				: allSearchPaths.length > 1 || (primaryWorkspaceRoot ? !arePathsEqual(primaryWorkspaceRoot, config.cwd) : true)
		const workspaceContext = {
			isMultiRootEnabled: config.isMultiRootEnabled || false,
			usedWorkspaceHint: anyUsedWorkspaceHint,
			resolvedToNonPrimary,
			resolutionMethod: (anyUsedWorkspaceHint
				? "hint"
				: allSearchPaths.length > 1
					? "path_detection"
					: "primary_fallback") as "hint" | "primary_fallback" | "path_detection",
		}

		// Capture workspace path resolution telemetry
		if (config.isMultiRootEnabled && config.workspaceManager) {
			const resolutionType = anyUsedWorkspaceHint
				? "hint_provided"
				: allSearchPaths.length > 1
					? "cross_workspace_search"
					: "fallback_to_primary"
		}

		// Execute searches in all relevant workspaces in parallel
		const searchPromises = allSearchPaths.map(({ absolutePath, workspaceName, workspaceRoot }) =>
			this.executeSearch(config, absolutePath, workspaceName, workspaceRoot, regex, filePattern, contextLines, [
				"!.*",
				"!**/.*",
			]),
		)

		// Wait for all searches to complete
		const searchStartTime = performance.now()
		const searchResults = await Promise.all(searchPromises)
		const searchDurationMs = performance.now() - searchStartTime

		// Format and combine results
		const results = this.formatSearchResults(config, searchResults, allSearchPaths)

		// Only reset after a successful operation so repeated failures
		// accumulate toward the yolo-mode mistake limit.
		// If ALL searches failed, increment the mistake counter.
		const anySucceeded = searchResults.some((result) => result.success)
		if (anySucceeded) {
			config.taskState.consecutiveMistakeCount = 0
		} else {
			config.taskState.consecutiveMistakeCount++
		}

		// Capture workspace search pattern telemetry
		if (config.isMultiRootEnabled && config.workspaceManager) {
			const searchType = anyUsedWorkspaceHint ? "targeted" : allSearchPaths.length > 1 ? "cross_workspace" : "primary_only"
			const resultsFound = searchResults.some((result) => result.resultCount > 0)

		}

		const sharedMessageProps = {
			tool: "searchFiles",
			paths: relPaths.map((p) => getReadablePath(config.cwd, p)),
			path: getReadablePath(config.cwd, relPaths[0] || ""),
			content: stripHashes(results),
			regex: regex,
			filePattern: filePattern,
			contextLines: contextLines,
			operationIsLocatedInWorkspace: (await Promise.all(relPaths.map((p) => isLocatedInWorkspace(p)))).every(Boolean),
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

			// Capture telemetry
		} else {
			// Manual approval flow
			const notificationMessage = `di wants to search files for ${regex}`

			// Show notification
			showNotificationForApproval(notificationMessage, config.autoApprovalSettings.enableNotifications)

			await config.callbacks.removeLastPartialMessageIfExistsWithType("say", "tool")

			const { didApprove } = await ToolResultUtils.askApprovalAndPushFeedback("tool", completeMessage, config)
			if (!didApprove) {
				return formatResponse.toolDenied()
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

		return results
	}
}
