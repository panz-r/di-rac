import { sendPartialMessageEvent } from "@core/controller/ui/subscribeToPartialMessage"
import { executeHook } from "@core/hooks/hook-executor"
import { getHookModelContext } from "@core/hooks/hook-model-context"
import { getHooksEnabledSafe } from "@core/hooks/hooks-utils"
import { formatResponse } from "@core/prompts/responses"
import { DiracAsk, DiracMessage, DiracSay, MultiCommandState } from "@shared/ExtensionMessage"
import { convertDiracMessageToProto } from "@shared/proto-conversions/dirac-message"
import { Logger } from "@shared/services/Logger"
import { DiracDefaultTool } from "@shared/tools"
import { DiracAskResponse } from "@shared/WebviewMessage"
import { createToolError } from "@shared/tool-response"
import pWaitFor from "p-wait-for"
import { TaskMessengerDependencies } from "./types/task-messenger"

interface UpsertResult {
	ts: number
	wasUpdate: boolean
}

export class TaskMessenger {
	constructor(private dependencies: TaskMessengerDependencies) {}

	private clearAskResponse() {
		const ts = this.dependencies.taskState
		ts.askResponse = undefined
		ts.askResponseText = undefined
		ts.askResponseImages = undefined
		ts.askResponseFiles = undefined
	}

	private findLastPartialIndex(
		diracMessages: DiracMessage[],
		type: "ask" | "say",
		subtype: DiracAsk | DiracSay,
	): number {
		for (let i = diracMessages.length - 1; i >= 0; i--) {
			const msg = diracMessages[i]
			if (msg.partial && msg.type === type && (msg.ask === subtype || msg.say === subtype)) {
				return i
			}
		}
		return -1
	}

	/**
	 * Handles the partial-message state machine shared by ask() and say():
	 *   partial=true  + existing → update in place, send proto event
	 *   partial=true  + no match → add new partial message
	 *   partial=false + existing → replace partial with complete, send proto event
	 *   partial=false + no match → add new complete message
	 *   partial=undefined         → add new non-partial message
	 */
	private async upsertPartialMessage(params: {
		type: "ask" | "say"
		subtype: DiracAsk | DiracSay
		partial: boolean | undefined
		fields: Record<string, any>
	}): Promise<UpsertResult> {
		const { type, subtype, partial, fields } = params
		const diracMessages = this.dependencies.messageStateHandler.getDiracMessages()

		if (partial !== undefined) {
			const idx = this.findLastPartialIndex(diracMessages, type, subtype)

			if (partial) {
				if (idx !== -1) {
					await this.dependencies.messageStateHandler.updateDiracMessage(idx, {
						...fields,
						partial: true,
						commandCompleted: false,
					})
					const protoMessage = convertDiracMessageToProto(diracMessages[idx])
					await sendPartialMessageEvent(protoMessage)
					await this.dependencies.postStateToWebview()
					return { ts: diracMessages[idx].ts, wasUpdate: true }
				}
				const ts = Date.now()
				this.dependencies.taskState.lastMessageTs = ts
				await this.dependencies.messageStateHandler.addToDiracMessages({
					ts,
					type,
					[type]: subtype,
					...fields,
					partial: true,
					commandCompleted: false,
				})
				await this.dependencies.postStateToWebview()
				return { ts, wasUpdate: false }
			}

			// partial=false: complete version of a previously partial message
			if (idx !== -1) {
				const ts = diracMessages[idx].ts
				this.dependencies.taskState.lastMessageTs = ts
				await this.dependencies.messageStateHandler.updateDiracMessage(idx, {
					...fields,
					partial: false,
					commandCompleted: false,
				})
				const protoMessage = convertDiracMessageToProto(diracMessages[idx])
				await sendPartialMessageEvent(protoMessage)
				await this.dependencies.postStateToWebview()
				return { ts, wasUpdate: true }
			}
			// New complete message (no existing partial to replace)
			const ts = Date.now()
			this.dependencies.taskState.lastMessageTs = ts
			await this.dependencies.messageStateHandler.addToDiracMessages({
				ts,
				type,
				[type]: subtype,
				...fields,
			})
			await this.dependencies.postStateToWebview()
			return { ts, wasUpdate: false }
		}

		// partial=undefined: new non-partial message
		const ts = Date.now()
		this.dependencies.taskState.lastMessageTs = ts
		await this.dependencies.messageStateHandler.addToDiracMessages({
			ts,
			type,
			[type]: subtype,
			...fields,
		})
		await this.dependencies.postStateToWebview()
		return { ts, wasUpdate: false }
	}

	async ask(
		type: DiracAsk,
		text?: string,
		partial?: boolean,
		multiCommandState?: MultiCommandState,
	): Promise<{
		response: DiracAskResponse
		text?: string
		images?: string[]
		files?: string[]
		askTs?: number
	}> {
		// Allow resume asks even when aborted to enable resume button after cancellation
		if (this.dependencies.taskState.abort && type !== "resume_task" && type !== "resume_completed_task") {
			throw new Error("Dirac instance aborted")
		}

		const askResponseSnapshot = () => ({
			response: this.dependencies.taskState.askResponse!,
			text: this.dependencies.taskState.askResponseText,
			images: this.dependencies.taskState.askResponseImages,
			files: this.dependencies.taskState.askResponseFiles,
		})

		if (partial) {
			const result = await this.upsertPartialMessage({
				type: "ask",
				subtype: type,
				partial: true,
				fields: { text, multiCommandState },
			})
			return { ...askResponseSnapshot(), askTs: result.ts }
		}

		this.clearAskResponse()

		const fields = partial === false ? { text, multiCommandState } : { text }
		const { ts: askTs } = await this.upsertPartialMessage({
			type: "ask",
			subtype: type,
			partial,
			fields,
		})

		// Notification hook marks that Dirac is waiting for user input.
		await this.runNotificationHook({
			event: "user_attention",
			source: type,
			message: text || "",
			waitingForUserInput: true,
		})

		await pWaitFor(
			() => {
				const response = this.dependencies.taskState.askResponse
				return response !== undefined || this.dependencies.taskState.lastMessageTs !== askTs
			},
			{ interval: 100 },
		)

		if (this.dependencies.taskState.lastMessageTs !== askTs) {
			Logger.debug("task_messenger", {
				event: "ask_abandoned",
				type,
				askTs,
				currentTs: this.dependencies.taskState.lastMessageTs,
			})
			throw new Error("Current ask promise was ignored")
		}

		const result = askResponseSnapshot()
		this.clearAskResponse()
		return result
	}

	async runNotificationHook(notification: {
		event: string
		source: string
		message: string
		waitingForUserInput: boolean
	}): Promise<void> {
		const hooksEnabled = getHooksEnabledSafe(this.dependencies.stateManager.getGlobalSettingsKey("hooksEnabled"))
		if (!hooksEnabled) {
			return
		}

		try {
			await executeHook({
				hookName: "Notification",
				hookInput: {
					notification,
				},
				isCancellable: false,
				say: async () => undefined,
				messageStateHandler: this.dependencies.messageStateHandler,
				taskId: this.dependencies.taskId,
				hooksEnabled,
				model: getHookModelContext(this.dependencies.api, this.dependencies.stateManager),
			})
		} catch (error) {
			Logger.error("[Notification Hook] Failed (non-fatal):", error)
		}
	}

	async handleWebviewAskResponse(askResponse: DiracAskResponse, text?: string, images?: string[], files?: string[]) {
		this.dependencies.taskState.askResponse = askResponse
		this.dependencies.taskState.askResponseText = text
		this.dependencies.taskState.askResponseImages = images
		this.dependencies.taskState.askResponseFiles = files
	}

	async say(
		type: DiracSay,
		text?: string,
		images?: string[],
		files?: string[],
		partial?: boolean,
		multiCommandState?: MultiCommandState,
	): Promise<number | undefined> {
		// Allow hook messages even when aborted to enable proper cleanup
		if (this.dependencies.taskState.abort && type !== "hook_status" && type !== "hook_output_stream") {
			throw new Error("Dirac instance aborted")
		}

		const providerInfo = this.dependencies.getCurrentProviderInfo()
		const modelInfo = {
			providerId: providerInfo.providerId,
			modelId: providerInfo.model.id,
			mode: providerInfo.mode,
		}

		const result = await this.upsertPartialMessage({
			type: "say",
			subtype: type,
			partial,
			fields: { text, images, files, modelInfo, multiCommandState },
		})

		// When replacing a partial with complete, return undefined
		return partial === false && result.wasUpdate ? undefined : result.ts
	}

	async sayAndCreateMissingParamError(toolName: DiracDefaultTool, paramName: string, relPath?: string) {
		// Clear any partial UI state for this tool
		await this.removeLastPartialMessageIfExistsWithType("say", "tool")
		await this.removeLastPartialMessageIfExistsWithType("ask", "tool")

		await this.say(
			"error",
			`Dirac tried to use ${toolName}${relPath ? ` for '${relPath.toPosix()}'` : ""} without providing a value for '${paramName}'. Retrying...`,
		)
		return formatResponse.formatToolErrorForLLM(createToolError("tool.unknownError", formatResponse.missingToolParameterError(paramName), "recoverable"))
	}

	async removeLastPartialMessageIfExistsWithType(type: "ask" | "say", askOrSay: DiracAsk | DiracSay) {
		const diracMessages = this.dependencies.messageStateHandler.getDiracMessages()
		const indexToRemove = this.findLastPartialIndex(diracMessages, type, askOrSay)

		if (indexToRemove !== -1) {
			const newMessages = [...diracMessages]
			newMessages.splice(indexToRemove, 1)
			this.dependencies.messageStateHandler.setDiracMessages(newMessages)
			await this.dependencies.messageStateHandler.saveDiracMessagesAndUpdateHistory()
			await this.dependencies.postStateToWebview()
		}
	}
}
