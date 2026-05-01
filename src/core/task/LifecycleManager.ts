import { executeHook } from "@core/hooks/hook-executor"
import { getHookModelContext } from "@core/hooks/hook-model-context"
import { getHooksEnabledSafe } from "@core/hooks/hooks-utils"
import { formatResponse } from "@core/prompts/responses"
import { generateOutlinesForChangedFiles } from "./resume-outline-refresh"
import { detectFileChanges, ensureTaskDirectoryExists, getSavedApiConversationHistory, getSavedDiracMessages } from "@core/storage/disk"
import { HostProvider } from "@hosts/host-provider"
import { processFilesIntoText } from "@integrations/misc/extract-text"
import { findLastIndex } from "@shared/array"
import { DiracApiReqInfo, DiracAsk } from "@shared/ExtensionMessage"
import { DiracContent, DiracImageContentBlock, DiracUserContent } from "@shared/messages/content"
import { ShowMessageType } from "@shared/proto/index.host"
import { Logger } from "@shared/services/Logger"
import { releaseTaskLock } from "./TaskLockUtils"
import { LifecycleManagerDependencies } from "./types/lifecycle-manager"
import { buildUserFeedbackContent } from "./utils/buildUserFeedbackContent"
import { printSessionSummary } from "./utils"

export class LifecycleManager {
	constructor(private dependencies: LifecycleManagerDependencies) {}

	public async startTask(task?: string, images?: string[], files?: string[]): Promise<void> {
		try {
			await this.dependencies.diracIgnoreController.initialize()
		} catch (error) {
			Logger.error("Failed to initialize DiracIgnoreController:", error)
		}
		this.dependencies.messageStateHandler.setDiracMessages([])
		this.dependencies.messageStateHandler.setApiConversationHistory([])

		await this.dependencies.postStateToWebview()

		await this.dependencies.say("task", task, images, files)

		this.dependencies.taskState.isInitialized = true

		const imageBlocks: DiracImageContentBlock[] = formatResponse.imageBlocks(images)

		const userContent: DiracUserContent[] = [
			{
				type: "text",
				text: `<task>\n${task}\n</task>`,
			},
			...imageBlocks,
		]

		if (files && files.length > 0) {
			const fileContentString = await processFilesIntoText(files)
			if (fileContentString) {
				userContent.push({
					type: "text",
					text: fileContentString,
				})
			}
		}

		const hooksEnabled = getHooksEnabledSafe(this.dependencies.stateManager.getGlobalSettingsKey("hooksEnabled"))
		if (hooksEnabled) {
			const taskStartResult = await executeHook({
				hookName: "TaskStart",
				hookInput: {
					taskStart: {
						taskMetadata: {
							taskId: this.dependencies.taskId,
							ulid: this.dependencies.ulid,
							initialTask: task || "",
						},
					},
				},
				isCancellable: true,
				say: this.dependencies.say.bind(this.dependencies),
				setActiveHookExecution: this.dependencies.hookManager.setActiveHookExecution.bind(this.dependencies.hookManager),
				clearActiveHookExecution: this.dependencies.hookManager.clearActiveHookExecution.bind(
					this.dependencies.hookManager,
				),
				messageStateHandler: this.dependencies.messageStateHandler,
				taskId: this.dependencies.taskId,
				hooksEnabled,
				model: getHookModelContext(this.dependencies.api, this.dependencies.stateManager),
			})

			if (taskStartResult.cancel === true) {
				await this.dependencies.hookManager.handleHookCancellation("TaskStart", taskStartResult.wasCancelled || false)
				await this.dependencies.cancelTask()
				return
			}

			if (taskStartResult.contextModification) {
				const contextText = taskStartResult.contextModification.trim()
				if (contextText) {
					userContent.push({
						type: "text",
						text: `<hook_context source="TaskStart">\n${contextText}\n</hook_context>`,
					})
				}
			}
		}

		if (this.dependencies.taskState.abort) {
			return
		}

		const userPromptHookResult = await this.dependencies.hookManager.runUserPromptSubmitHook(userContent, "initial_task")

		if (this.dependencies.taskState.abort) {
			return
		}

		if (userPromptHookResult.cancel === true) {
			await this.dependencies.hookManager.handleHookCancellation(
				"UserPromptSubmit",
				userPromptHookResult.wasCancelled ?? false,
			)
			await this.dependencies.cancelTask()
			return
		}

		if (userPromptHookResult.contextModification) {
			userContent.push({
				type: "text",
				text: `<hook_context source="UserPromptSubmit">\n${userPromptHookResult.contextModification}\n</hook_context>`,
			})
		}

		try {
			await this.dependencies.recordEnvironment()
		} catch (error) {
			Logger.error("Failed to record environment metadata:", error)
		}

		// Start tree-sitter daemon (non-blocking — falls back to WASM if unavailable)
		try {
			await this.dependencies.analyzer.start()
		} catch (error) {
			Logger.error("Failed to start analyzer daemon:", error)
		}

		await this.dependencies.initiateTaskLoop(userContent)
	}

	public async resumeTaskFromHistory() {
		try {
			await this.dependencies.diracIgnoreController.initialize()
		} catch (error) {
			Logger.error("Failed to initialize DiracIgnoreController:", error)
		}

		const savedDiracMessages = await getSavedDiracMessages(this.dependencies.taskId)

		const lastRelevantMessageIndex = findLastIndex(
			savedDiracMessages,
			(m) => !(m.ask === "resume_task" || m.ask === "resume_completed_task"),
		)
		if (lastRelevantMessageIndex !== -1) {
			savedDiracMessages.splice(lastRelevantMessageIndex + 1)
		}

		const lastApiReqStartedIndex = findLastIndex(savedDiracMessages, (m) => m.type === "say" && m.say === "api_req_started")
		if (lastApiReqStartedIndex !== -1) {
			const lastApiReqStarted = savedDiracMessages[lastApiReqStartedIndex]
			const { cost, cancelReason }: DiracApiReqInfo = JSON.parse(lastApiReqStarted.text || "{}")
			if (cost === undefined && cancelReason === undefined) {
				savedDiracMessages.splice(lastApiReqStartedIndex, 1)
			}
		}

		await this.dependencies.messageStateHandler.overwriteDiracMessages(savedDiracMessages)

		const savedApiConversationHistory = await getSavedApiConversationHistory(this.dependencies.taskId)
		this.dependencies.messageStateHandler.setApiConversationHistory(savedApiConversationHistory as any)

		await ensureTaskDirectoryExists(this.dependencies.taskId)

		const lastDiracMessage = this.dependencies.messageStateHandler
			.getDiracMessages()
			.slice()
			.reverse()
			.find((m) => !(m.ask === "resume_task" || m.ask === "resume_completed_task"))

		let askType: DiracAsk
		if (lastDiracMessage?.ask === "completion_result") {
			askType = "resume_completed_task"
		} else {
			askType = "resume_task"
		}

		this.dependencies.taskState.isInitialized = true
		this.dependencies.taskState.abort = false

		const { response, text, images, files } = await this.dependencies.ask(askType)

		const newUserContent: DiracContent[] = []

		const hooksEnabled = getHooksEnabledSafe(this.dependencies.stateManager.getGlobalSettingsKey("hooksEnabled"))
		if (hooksEnabled) {
			const diracMessages = this.dependencies.messageStateHandler.getDiracMessages()
			const taskResumeResult = await executeHook({
				hookName: "TaskResume",
				hookInput: {
					taskResume: {
						taskMetadata: {
							taskId: this.dependencies.taskId,
							ulid: this.dependencies.ulid,
						},
						previousState: {
							lastMessageTs: lastDiracMessage?.ts?.toString() || "",
							messageCount: diracMessages.length.toString(),
							conversationHistoryDeleted: (
								this.dependencies.taskState.conversationHistoryDeletedRange !== undefined
							).toString(),
						},
					},
				},
				isCancellable: true,
				say: this.dependencies.say.bind(this.dependencies),
				setActiveHookExecution: this.dependencies.hookManager.setActiveHookExecution.bind(this.dependencies.hookManager),
				clearActiveHookExecution: this.dependencies.hookManager.clearActiveHookExecution.bind(
					this.dependencies.hookManager,
				),
				messageStateHandler: this.dependencies.messageStateHandler,
				taskId: this.dependencies.taskId,
				hooksEnabled,
				model: getHookModelContext(this.dependencies.api, this.dependencies.stateManager),
			})

			if (taskResumeResult.cancel === true) {
				await this.dependencies.hookManager.handleHookCancellation("TaskResume", taskResumeResult.wasCancelled || false)
				await this.dependencies.cancelTask()
				return
			}

			if (taskResumeResult.contextModification) {
				newUserContent.push({
					type: "text",
					text: `<hook_context source="TaskResume" type="general">\n${taskResumeResult.contextModification}\n</hook_context>`,
				})
			}
		}

		if (this.dependencies.taskState.abort) {
			return
		}

		let responseText: string | undefined
		let responseImages: string[] | undefined
		let responseFiles: string[] | undefined
		if (response === "messageResponse" || text || (images && images.length > 0) || (files && files.length > 0)) {
			await this.dependencies.say("user_feedback", text, images, files)
			responseText = text
			responseImages = images
			responseFiles = files
		}

		const existingApiConversationHistory = this.dependencies.messageStateHandler.getApiConversationHistory()
		let modifiedOldUserContent: DiracContent[]
		let modifiedApiConversationHistory: any[]
		if (existingApiConversationHistory.length > 0) {
			const lastMessage = existingApiConversationHistory[existingApiConversationHistory.length - 1]
			if (lastMessage.role === "assistant") {
				modifiedApiConversationHistory = [...existingApiConversationHistory]
				modifiedOldUserContent = []
			} else if (lastMessage.role === "user") {
				const existingUserContent: DiracContent[] = Array.isArray(lastMessage.content)
					? lastMessage.content
					: [{ type: "text", text: lastMessage.content }]
				modifiedApiConversationHistory = existingApiConversationHistory.slice(0, -1)
				modifiedOldUserContent = [...existingUserContent]
			} else {
				throw new Error("Unexpected: Last message is not a user or assistant message")
			}
		} else {
			modifiedApiConversationHistory = []
			modifiedOldUserContent = []
		}

		newUserContent.push(...modifiedOldUserContent)

		const agoText = (() => {
			const timestamp = lastDiracMessage?.ts ?? Date.now()
			const now = Date.now()
			const diff = now - timestamp
			const minutes = Math.floor(diff / 60000)
			const hours = Math.floor(minutes / 60)
			const days = Math.floor(hours / 24)
			if (days > 0) return `${days} day${days > 1 ? "s" : ""} ago`
			if (hours > 0) return `${hours} hour${hours > 1 ? "s" : ""} ago`
			if (minutes > 0) return `${minutes} minute${minutes > 1 ? "s" : ""} ago`
			return "just now"
		})()

		const wasRecent = lastDiracMessage?.ts && Date.now() - lastDiracMessage.ts < 30_000
		const pendingContextWarning = await this.dependencies.fileContextTracker.retrieveAndClearPendingFileContextWarning()
		const hasPendingFileContextWarnings = pendingContextWarning && pendingContextWarning.length > 0
		const mode = this.dependencies.stateManager.getGlobalSettingsKey("mode")
		const [taskResumptionMessage, userResponseMessage] = formatResponse.taskResumption(
			mode === "plan" ? "plan" : "act",
			agoText,
			this.dependencies.cwd,
			wasRecent,
			responseText,
			hasPendingFileContextWarnings,
		)

		if (taskResumptionMessage !== "") {
			newUserContent.push({
				type: "text",
				text: taskResumptionMessage,
			})
		}
		if (userResponseMessage !== "") {
			newUserContent.push({
				type: "text",
				text: userResponseMessage,
			})
		}

		// Detect filesystem changes since last save
		const fileChanges = await detectFileChanges(this.dependencies.taskId, this.dependencies.cwd)
		if (fileChanges.changed.length > 0 || fileChanges.deleted.length > 0) {
			const notice = formatResponse.filesystemStateNotice(fileChanges.changed, fileChanges.deleted)
			newUserContent.push({ type: "text", text: notice })

			// Pre-refresh outlines for changed files
			if (fileChanges.changed.length > 0) {
				const outlineText = await generateOutlinesForChangedFiles(fileChanges.changed, this.dependencies.cwd, this.dependencies.analyzer)
				if (outlineText) {
					newUserContent.push({ type: "text", text: outlineText })
				}
			}
		} else if (!wasRecent) {
			// Lightweight disclaimer when no manifest changes detected
			newUserContent.push({
				type: "text",
				text: "[SYSTEM: This session was restored from a previous savepoint. The filesystem may have been modified independently since then.]",
			})
		}

		if (responseImages && responseImages.length > 0) {
			newUserContent.push(...formatResponse.imageBlocks(responseImages))
		}

		if (responseFiles && responseFiles.length > 0) {
			const fileContentString = await processFilesIntoText(responseFiles)
			if (fileContentString) {
				newUserContent.push({
					type: "text",
					text: fileContentString,
				})
			}
		}

		if (pendingContextWarning && pendingContextWarning.length > 0) {
			const fileContextWarning = formatResponse.fileContextWarning(pendingContextWarning)
			newUserContent.push({
				type: "text",
				text: fileContextWarning,
			})
		}

		const userFeedbackContent = await buildUserFeedbackContent(responseText, responseImages, responseFiles)
		const userPromptHookResult = await this.dependencies.hookManager.runUserPromptSubmitHook(userFeedbackContent, "resume")

		if (this.dependencies.taskState.abort) {
			return
		}

		if (userPromptHookResult.cancel === true) {
			await this.dependencies.cancelTask()
			return
		}

		if (userPromptHookResult.contextModification) {
			newUserContent.push({
				type: "text",
				text: `<hook_context source="UserPromptSubmit">\n${userPromptHookResult.contextModification}\n</hook_context>`,
			})
		}

		try {
			await this.dependencies.recordEnvironment()
		} catch (error) {
			Logger.error("Failed to record environment metadata on resume:", error)
		}

		await this.dependencies.messageStateHandler.overwriteApiConversationHistory(modifiedApiConversationHistory)

		// Start tree-sitter daemon for resumed tasks
		try {
			await this.dependencies.analyzer.start()
		} catch (error) {
			Logger.error("Failed to start analyzer daemon on resume:", error)
		}

		await this.dependencies.initiateTaskLoop(newUserContent)
	}

	public async abortTask() {
		try {
			const shouldRunTaskCancelHook = await this.dependencies.hookManager.shouldRunTaskCancelHook()

			this.dependencies.taskState.abort = true

			const activeHook = await this.dependencies.hookManager.getActiveHookExecution()
			if (activeHook) {
				try {
					await this.dependencies.hookManager.cancelHookExecution()
					await this.dependencies.hookManager.clearActiveHookExecution()
				} catch (error) {
					Logger.error("Failed to cancel hook during task abort", error)
					await this.dependencies.hookManager.clearActiveHookExecution()
				}
			}

			if (this.dependencies.commandExecutor.hasActiveBackgroundCommand()) {
				try {
					await this.dependencies.commandExecutor.cancelBackgroundCommand()
				} catch (error) {
					Logger.error("Failed to cancel background command during task abort", error)
				}
			}

			const hooksEnabled = getHooksEnabledSafe(this.dependencies.stateManager.getGlobalSettingsKey("hooksEnabled"))
			if (hooksEnabled && shouldRunTaskCancelHook) {
				try {
					await executeHook({
						hookName: "TaskCancel",
						hookInput: {
							taskCancel: {
								taskMetadata: {
									taskId: this.dependencies.taskId,
									ulid: this.dependencies.ulid,
									completionStatus: this.dependencies.taskState.abandoned ? "abandoned" : "cancelled",
								},
							},
						},
						isCancellable: false,
						say: this.dependencies.say.bind(this.dependencies),
						messageStateHandler: this.dependencies.messageStateHandler,
						taskId: this.dependencies.taskId,
						hooksEnabled,
						model: getHookModelContext(this.dependencies.api, this.dependencies.stateManager),
					})

					const lastDiracMessage = this.dependencies.messageStateHandler
						.getDiracMessages()
						.slice()
						.reverse()
						.find((m) => !(m.ask === "resume_task" || m.ask === "resume_completed_task"))

					let askType: DiracAsk
					if (lastDiracMessage?.ask === "completion_result") {
						askType = "resume_completed_task"
					} else {
						askType = "resume_task"
					}

					// Fire-and-forget: the ask may resume the task but abortTask's finally block
					// will run immediately. This is acceptable because the resumed task will
					// re-acquire the lock and start fresh.
					this.dependencies.ask(askType).catch((error) => {
						Logger.log("[TaskCancel] Resume ask failed (task may have been cleared):", error)
					})
				} catch (error) {
					Logger.error("[TaskCancel Hook] Failed (non-fatal):", error)
				}
			}

			try {
				await this.dependencies.messageStateHandler.saveDiracMessagesAndUpdateHistory()
				await this.dependencies.postStateToWebview()
			} catch (error) {
				Logger.error("Failed to post state after setting abort flag", error)
			}

			// Print session summary before disposing resources
			try {
				const summaryData = this.dependencies.getSessionSummaryData?.()
				if (summaryData) {
					printSessionSummary({
						taskId: this.dependencies.taskId,
						messages: this.dependencies.messageStateHandler.getDiracMessages(),
						totalToolCallCount: this.dependencies.taskState.totalToolCallCount,
						taskStartTimeMs: this.dependencies.taskState.taskStartTimeMs,
						recoveryEngine: summaryData.recoveryEngine,
					})
				}
			} catch {
				// non-fatal
			}

			// Shut down tree-sitter daemon
			try {
				await this.dependencies.analyzer.shutdown()
			} catch {
				// non-fatal
			}

			this.dependencies.terminalManager.disposeAll()
			this.dependencies.urlContentFetcher.closeBrowser()
			await this.dependencies.browserSession.dispose()
			this.dependencies.diracIgnoreController.dispose()
			this.dependencies.fileContextTracker.dispose()
			this.dependencies.messageStateHandler.dispose()
			await this.dependencies.diffViewProvider.revertChanges()
		} finally {
			if (this.dependencies.taskState.taskLockAcquired) {
				try {
					await releaseTaskLock(this.dependencies.taskId)
					this.dependencies.taskState.taskLockAcquired = false
					Logger.info(`[Task ${this.dependencies.taskId}] Task lock released`)
				} catch (error) {
					Logger.error(`[Task ${this.dependencies.taskId}] Failed to release task lock:`, error)
				}
			}

			try {
				await this.dependencies.postStateToWebview()
			} catch (error) {
				Logger.error("Failed to post final state after abort", error)
			}
		}
	}
}
