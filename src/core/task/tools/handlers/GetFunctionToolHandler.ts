import { ToolUse } from "@core/assistant-message"
import { resolveWorkspacePath } from "@core/workspace"
import { ASTAnchorBridge } from "@utils/ASTAnchorBridge"
import { getReadablePath, isLocatedInWorkspace } from "@utils/path"
import { formatResponse } from "@/core/prompts/responses"
import { DiracDefaultTool } from "@/shared/tools"
import { DiracAssistantToolUseBlock, DiracStorageMessage, DiracUserToolResultContentBlock } from "@/shared/messages"
import type { ToolResponse } from "../../index"
import { showNotificationForApproval } from "../../utils"
import type { IFullyManagedTool } from "../ToolExecutorCoordinator"
import type { ToolValidator } from "../ToolValidator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"
import { ToolResultUtils } from "../utils/ToolResultUtils"

export class GetFunctionToolHandler implements IFullyManagedTool {
	readonly name = DiracDefaultTool.GET_FUNCTION

	constructor(private validator: ToolValidator) {}

	getDescription(block: ToolUse): string {
		const functionNames = Array.isArray(block.params.function_names) ? block.params.function_names : (block.params.function_names ? [block.params.function_names as string] : [])
		const relPaths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : (block.params.path ? [block.params.path as string] : []))
		return `${block.name} ${functionNames.join(", ")} in ${relPaths.join(" ")}`
	}
	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		const relPaths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : (block.params.path ? [block.params.path as string] : []))
		const functionNames = Array.isArray(block.params.function_names) ? block.params.function_names : (block.params.function_names ? [block.params.function_names as string] : [])

		const config = uiHelpers.getConfig()
		if (config.isSubagentExecution) {
			return
		}

		const sharedMessageProps = {
			tool: "getFunction",
			paths: relPaths.map((p) => getReadablePath(config.cwd, uiHelpers.removeClosingTag(block, "paths", p))),
			functionNames: functionNames.map((name) => uiHelpers.removeClosingTag(block, "function_names", name)),
		}

		const partialMessage = JSON.stringify(sharedMessageProps)

		const shouldAutoApprove =
			config.isSubagentExecution ||
			(await Promise.all(relPaths.map((p) => uiHelpers.shouldAutoApproveToolWithPath(block.name, p)))).every(Boolean)

		if (shouldAutoApprove) {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("ask", "tool")
			await uiHelpers.say("tool", partialMessage, undefined, undefined, block.partial)
		} else {
			await uiHelpers.removeLastPartialMessageIfExistsWithType("say", "tool")
			await uiHelpers.ask("tool", partialMessage, block.partial).catch(() => {})
		}
	}

		private extractLastKnownHashFromHistory(
		history: DiracStorageMessage[],
		targetPath: string,
		functionName: string,
	): string | undefined {
		// Iterate backwards to find the most recent get_function for this exact path and function
		for (let i = history.length - 1; i >= 0; i--) {
			const message = history[i]

			// Find assistant messages containing tool calls
			if (message.role === "assistant" && Array.isArray(message.content)) {
				for (const block of message.content) {
					if (block.type === "tool_use") {
						const toolUseBlock = block as unknown as DiracAssistantToolUseBlock
						const input = toolUseBlock.input as any
						const hasPathMatch =
							input?.path === targetPath || (Array.isArray(input?.paths) && input.paths.includes(targetPath))
						const hasFunctionMatch =
							input?.function_names && Array.isArray(input.function_names) && input.function_names.includes(functionName)

						if (toolUseBlock.name === this.name && hasPathMatch && hasFunctionMatch) {
							const toolUseId = toolUseBlock.id

							// The tool_result is almost always in the immediately following 'user' message
							const nextMessage = history[i + 1]
							if (nextMessage && nextMessage.role === "user" && Array.isArray(nextMessage.content)) {
								const resultBlock = nextMessage.content.find(
									(c: any) =>
										c.type === "tool_result" && (c as unknown as DiracUserToolResultContentBlock).tool_use_id === toolUseId,
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
										// Match the exact hash string we output for this specific function
										// We look for the section starting with "path::functionName"
										const escapedPath = targetPath.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
										const escapedFuncName = functionName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
										const sectionRegex = new RegExp(
											`${escapedPath}::${escapedFuncName}\\n\\[Function Hash: ([a-f0-9]+)\\]`,
										)
										const match = text.match(sectionRegex)
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


		async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const relPaths = Array.isArray(block.params.paths) ? block.params.paths : (block.params.paths ? [block.params.paths as string] : (block.params.path ? [block.params.path as string] : []))
		const functionNames = Array.isArray(block.params.function_names) ? block.params.function_names : (block.params.function_names ? [block.params.function_names as string] : [])

		const apiConfig = config.services.stateManager.getApiConfiguration()
		const currentMode = config.services.stateManager.getGlobalSettingsKey("mode")
		const provider = (currentMode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider) as string

		if (relPaths.length === 0 || functionNames.length === 0) {
			config.taskState.consecutiveMistakeCount++
			return await config.callbacks.sayAndCreateMissingParamError(
				this.name,
				relPaths.length === 0 ? "paths" : "function_names",
			)
		}

		const history = config.messageState.getApiConversationHistory() || []
		const results: string[] = []
		const foundNamesTotal = new Set<string>()
		let hasError = false

		for (const relPath of relPaths) {
			try {
				const pathResult = resolveWorkspacePath(config, relPath, "GetFunctionToolHandler.execute")
				const { absolutePath, displayPath } =
					typeof pathResult === "string" ? { absolutePath: pathResult, displayPath: relPath } : pathResult

				const result = await ASTAnchorBridge.getFunctions(
					absolutePath,
					displayPath,
					functionNames,
					config.services.diracIgnoreController,
					config.ulid,
					config.services.analyzer,
				)

				if (result) {
					// We need to split the formattedContent by our separator to handle each function individually
					const individualFuncs = result.formattedContent.split("\n\n---\n\n")
					const processedFuncs: string[] = []

					for (const funcContent of individualFuncs) {
						// Extract function name from the first line (relPath::functionName)
						const firstLine = funcContent.split("\n")[0]
						const functionName = firstLine.split("::")[1]

						if (functionName) {
							const currentHashMatch = funcContent.match(/\[Function Hash: ([a-f0-9]+)\]/)
							const currentHash = currentHashMatch ? currentHashMatch[1] : undefined
							const lastKnownHash = this.extractLastKnownHashFromHistory(history, relPath, functionName)

							if (currentHash && lastKnownHash === currentHash) {
								processedFuncs.push(`${firstLine}\nno changes have been made to the function since your last read (Hash: ${currentHash})`)
							} else {
								processedFuncs.push(funcContent)
							}
						} else {
							processedFuncs.push(funcContent)
						}
					}

					results.push(processedFuncs.join("\n\n---\n\n"))
					for (const name of result.foundNames) {
						foundNamesTotal.add(name)
					}
					if (result.foundNames.length === 0) {
						hasError = true
					}
				} else {
					results.push(`None of the requested functions (${functionNames.join(", ")}) were found in ${relPath}`)
					hasError = true
				}
			} catch (error) {
				hasError = true
				const errorMessage = error instanceof Error ? error.message : String(error)
				results.push(`Error extracting functions from ${relPath}: ${errorMessage}`)
			}
		}

		const result = results.join("\n\n" + "=".repeat(20) + "\n\n")

		const missingNamesTotal = functionNames.filter((name) => !foundNamesTotal.has(name))
		let finalResult = result
		if (missingNamesTotal.length > 0) {
			finalResult += `\n\nNote: The following functions were not found in any of the provided files: ${missingNamesTotal.join(", ")}`
		}

		if (hasError && foundNamesTotal.size === 0) {
			config.taskState.consecutiveMistakeCount++
		} else {
			config.taskState.consecutiveMistakeCount = 0
		}

		const sharedMessageProps = {
			tool: "getFunction",
			paths: relPaths.map((p) => getReadablePath(config.cwd, p)),
			functionNames,
			foundFunctionNames: Array.from(foundNamesTotal),
			operationIsLocatedInWorkspace: (await Promise.all(relPaths.map((p) => isLocatedInWorkspace(p)))).every(Boolean),
			path: relPaths[0],
		}

		const completeMessage = JSON.stringify(sharedMessageProps)

		const shouldAutoApprove =
			config.isSubagentExecution ||
			(await Promise.all(relPaths.map((p) => config.callbacks.shouldAutoApproveToolWithPath(block.name, p)))).every(Boolean)

		if (shouldAutoApprove) {
			if (!config.isSubagentExecution) {
				await config.callbacks.removeLastPartialMessageIfExistsWithType("ask", "tool")
				await config.callbacks.say("tool", completeMessage, undefined, undefined, false)
			}

		} else {
			const notificationMessage = `di wants to extract ${functionNames.length} function(s) from ${relPaths.length} file(s)`
			showNotificationForApproval(notificationMessage, config.autoApprovalSettings.enableNotifications)

			await config.callbacks.removeLastPartialMessageIfExistsWithType("say", "tool")
			await config.callbacks.removeLastPartialMessageIfExistsWithType("ask", "tool")

			const { didApprove } = await ToolResultUtils.askApprovalAndPushFeedback("tool", completeMessage, config)
			if (!didApprove) {
				return formatResponse.toolDenied()
			}
		}

		return finalResult
	}
}
