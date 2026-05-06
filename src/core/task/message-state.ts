import { EventEmitter } from "events"
import getFolderSize from "get-folder-size"
import Mutex from "p-mutex"
import { findLastIndex } from "@/shared/array"
import { combineApiRequests } from "@/shared/combineApiRequests"
import { combineCommandSequences } from "@/shared/combineCommandSequences"
import { DiracMessage } from "@/shared/ExtensionMessage"
import { getApiMetrics } from "@/shared/getApiMetrics"
import { HistoryItem } from "@/shared/HistoryItem"
import { DiracStorageMessage } from "@/shared/messages/content"
import { Logger } from "@/shared/services/Logger"
import { getCwd, getDesktopDir } from "@/utils/path"
import { ensureTaskDirectoryExists, saveApiConversationHistory, saveDiracMessages } from "../storage/disk"
import { TaskState } from "./TaskState"

// Event types for diracMessages changes
export type DiracMessageChangeType = "add" | "update" | "delete" | "set"

export interface DiracMessageChange {
	type: DiracMessageChangeType
	/** The full array after the change */
	messages: DiracMessage[]
	/** The affected index (for add/update/delete) */
	index?: number
	/** The new/updated message (for add/update) */
	message?: DiracMessage
	/** The old message before change (for update/delete) */
	previousMessage?: DiracMessage
	/** The entire previous array (for set) */
	previousMessages?: DiracMessage[]
}

// Strongly-typed event emitter interface
export interface MessageStateHandlerEvents {
	diracMessagesChanged: [change: DiracMessageChange]
}

interface MessageStateHandlerParams {
	taskId: string
	ulid: string
	taskIsFavorited?: boolean
	workspaceRootPath?: string
	updateTaskHistory: (historyItem: HistoryItem) => Promise<HistoryItem[]>
	taskState: TaskState
}

export class MessageStateHandler extends EventEmitter<MessageStateHandlerEvents> {
	private workspaceRootPath?: string
	private apiConversationHistory: DiracStorageMessage[] = []
	private diracMessages: DiracMessage[] = []
	private taskIsFavorited: boolean
	private updateTaskHistory: (historyItem: HistoryItem) => Promise<HistoryItem[]>
	private taskId: string
	private ulid: string
	private taskState: TaskState

	// Debounce history updates to avoid excessive folder size calculations and disk I/O
	private historyUpdateTimer: NodeJS.Timeout | null = null
	private lastHistoryUpdateTs = 0
	private readonly HISTORY_UPDATE_DEBOUNCE_MS = 2000

	// Debounce disk saves to reduce I/O overhead during rapid updates
	private diskSaveTimer: NodeJS.Timeout | null = null
	private savingInProgress = false
	private lastDiskSaveTs = 0
	private readonly DISK_SAVE_DEBOUNCE_MS = 1000

	// Mutex to prevent concurrent state modifications (RC-4)
	// Protects against data loss from race conditions when multiple
	// operations try to modify message state simultaneously
	// This follows the same pattern as Task.stateMutex for consistency
	private disposed = false

	private stateMutex = new Mutex()

	constructor(params: MessageStateHandlerParams) {
		super()
		this.taskId = params.taskId
		this.ulid = params.ulid
		this.taskState = params.taskState
		this.taskIsFavorited = params.taskIsFavorited ?? false
		this.workspaceRootPath = params.workspaceRootPath
		this.updateTaskHistory = params.updateTaskHistory
	}

	/**
	 * Emit a diracMessagesChanged event with the change details
	 */
	private emitDiracMessagesChanged(change: DiracMessageChange): void {
		this.emit("diracMessagesChanged", change)
	}

	/**
	 * Execute function with exclusive lock on message state
	 * Use this for ANY state modification to prevent race conditions
	 * This follows the same pattern as Task.withStateLock for consistency
	 */
	private async withStateLock<T>(fn: () => T | Promise<T>): Promise<T> {
		return await this.stateMutex.withLock(fn)
	}

	getApiConversationHistory(): DiracStorageMessage[] {
		return this.apiConversationHistory
	}

	setApiConversationHistory(newHistory: DiracStorageMessage[]): void {
		this.apiConversationHistory = newHistory
	}

	getDiracMessages(): DiracMessage[] {
		return this.diracMessages
	}

	// Unprotected by mutex intentionally - only called during task initialization
	// when no concurrent modifications are possible. For active sessions use overwriteDiracMessages().
	setDiracMessages(newMessages: DiracMessage[]) {
		const previousMessages = this.diracMessages
		this.diracMessages = newMessages
		this.emitDiracMessagesChanged({
			type: "set",
			messages: this.diracMessages,
			previousMessages,
		})
	}

	/**
	 * Internal method to save messages and update history (without mutex protection)
	 * This is used by methods that already hold the stateMutex lock
	 * Should NOT be called directly - use saveDiracMessagesAndUpdateHistory() instead
	 */
	/**
	 * Internal method to save messages (without mutex protection)
	 */
	private async saveDiracMessagesInternal(): Promise<void> {
		try {
			await saveDiracMessages(this.taskId, this.diracMessages)
		} catch (error) {
			Logger.error("Failed to save dirac messages:", error)
		}
	}

	private debouncedSaveDiracMessages(): void {
		if (this.savingInProgress || this.diskSaveTimer) {
			return
		}

		const doSave = async () => {
			this.savingInProgress = true
			try {
				await this.saveDiracMessagesInternal()
			} finally {
				this.savingInProgress = false
			}
		}

		const now = Date.now()
		if (now - this.lastDiskSaveTs > this.DISK_SAVE_DEBOUNCE_MS) {
			this.lastDiskSaveTs = now
			if (!this.disposed) doSave().catch((err) => Logger.error("Failed to save messages:", err))
			return
		}

		this.diskSaveTimer = setTimeout(() => {
			this.diskSaveTimer = null
			this.lastDiskSaveTs = Date.now()
			if (!this.disposed) doSave().catch((err) => Logger.error("Failed to save messages:", err))
		}, this.DISK_SAVE_DEBOUNCE_MS)
	}

	/**
	 * Update task history with current state.
	 * This can be slow due to folder size calculation, so it should be called
	 * outside of the stateMutex lock when possible.
	 */
	private async updateTaskHistoryInternal(): Promise<void> {
		if (this.disposed) return
		try {
			// Capture state needed for history update
			// Note: we don't hold the lock here, but these are mostly immutable or
			// fine to have slight inconsistencies in the history summary.
			const messages = [...this.diracMessages]
			if (messages.length === 0) return

			const apiMetrics = getApiMetrics(combineApiRequests(combineCommandSequences(messages.slice(1))))
			const taskMessage = messages[0]
			const lastRelevantMessage =
				messages[
					findLastIndex(
						messages,
						(message) => !(message.ask === "resume_task" || message.ask === "resume_completed_task"),
					)
				] || messages[messages.length - 1]

			const lastModelInfo = [...this.apiConversationHistory].reverse().find((msg) => msg.modelInfo !== undefined)
			const taskDir = await ensureTaskDirectoryExists(this.taskId)
			
			// Slow operation: get folder size
			let taskDirSize = 0
			try {
				taskDirSize = await getFolderSize.loose(taskDir)
			} catch (error) {
				Logger.error("Failed to get task directory size:", taskDir, error)
			}

			const cwd = await getCwd(getDesktopDir())

			await this.updateTaskHistory({
				id: this.taskId,
				ulid: this.ulid,
				ts: lastRelevantMessage.ts,
				task: taskMessage.text ?? "",
				tokensIn: apiMetrics.totalTokensIn,
				tokensOut: apiMetrics.totalTokensOut,
				cacheWrites: apiMetrics.totalCacheWrites,
				cacheReads: apiMetrics.totalCacheReads,
				size: taskDirSize,
				cwdOnTaskInitialization: cwd,
				conversationHistoryDeletedRange: this.taskState.conversationHistoryDeletedRange,
				isFavorited: this.taskIsFavorited,
				workspaceRootPath: this.workspaceRootPath,
				modelId: lastModelInfo?.modelInfo?.modelId,
			})
		} catch (error) {
			Logger.error("Failed to update task history:", error)
		}
	}

	/**
	 * Save dirac messages and update task history (public API with mutex protection)
	 * This is the main entry point for saving message state from external callers.
	 * The mutex protects the scheduling decision; the debounce logic handles coalescing.
	 */
	async saveDiracMessagesAndUpdateHistory(): Promise<void> {
		await this.withStateLock(async () => {
			this.debouncedSaveDiracMessages()
		})
		this.debouncedUpdateTaskHistory()
	}

	private debouncedUpdateTaskHistory(): void {
		const now = Date.now()

		// If we already have a timer, just let it run
		if (this.historyUpdateTimer) {
			return
		}

		// If we haven't updated in a while, do it now
		if (now - this.lastHistoryUpdateTs > this.HISTORY_UPDATE_DEBOUNCE_MS) {
			this.lastHistoryUpdateTs = now
			this.updateTaskHistoryInternal().catch((err) => Logger.error("Failed to update history:", err))
			return
		}

		// Otherwise, schedule it
		this.historyUpdateTimer = setTimeout(() => {
			this.historyUpdateTimer = null
			this.lastHistoryUpdateTs = Date.now()
			this.updateTaskHistoryInternal().catch((err) => Logger.error("Failed to update history:", err))
		}, this.HISTORY_UPDATE_DEBOUNCE_MS)
	}

	dispose(): void {
		this.disposed = true
		if (this.historyUpdateTimer) {
			clearTimeout(this.historyUpdateTimer)
			this.historyUpdateTimer = null
		}
		if (this.diskSaveTimer) {
			clearTimeout(this.diskSaveTimer)
			this.diskSaveTimer = null
		}
	}

	async addToApiConversationHistory(message: DiracStorageMessage) {
		// Protect with mutex to prevent concurrent modifications from corrupting data (RC-4)
		return await this.withStateLock(async () => {
			this.apiConversationHistory.push(message)
			await saveApiConversationHistory(this.taskId, this.apiConversationHistory)
		})
	}

	async overwriteApiConversationHistory(newHistory: DiracStorageMessage[]): Promise<void> {
		// Protect with mutex to prevent concurrent modifications from corrupting data (RC-4)
		return await this.withStateLock(async () => {
			this.apiConversationHistory = newHistory
			await saveApiConversationHistory(this.taskId, this.apiConversationHistory)
		})
	}

	/**
	 * Add a new message to diracMessages array with proper index tracking
	 * CRITICAL: This entire operation must be atomic to prevent race conditions (RC-4)
	 * The conversationHistoryIndex must be set correctly based on the current state,
	 * and the message must be added and saved without any interleaving operations
	 */
	async addToDiracMessages(message: DiracMessage) {
		await this.withStateLock(async () => {
			// these values allow us to reconstruct the conversation history at the time this dirac message was created
			// it's important that apiConversationHistory is initialized before we add dirac messages
			// -1 signals "no prior conversation history" — safety fallback if history is empty
			message.conversationHistoryIndex = this.apiConversationHistory.length - 1
			message.conversationHistoryDeletedRange = this.taskState.conversationHistoryDeletedRange
			const index = this.diracMessages.length
			this.diracMessages.push(message)
			this.emitDiracMessagesChanged({
				type: "add",
				messages: this.diracMessages,
				index,
				message,
			})
			this.debouncedSaveDiracMessages()
		})
		this.debouncedUpdateTaskHistory()
	}

	/**
	 * Replace the entire diracMessages array with new messages
	 * Protected by mutex to prevent concurrent modifications (RC-4)
	 */
	async overwriteDiracMessages(newMessages: DiracMessage[]) {
		await this.withStateLock(async () => {
			const previousMessages = this.diracMessages
			this.diracMessages = newMessages
			this.emitDiracMessagesChanged({
				type: "set",
				messages: this.diracMessages,
				previousMessages,
			})
			this.debouncedSaveDiracMessages()
		})
		this.debouncedUpdateTaskHistory()
	}

	/**
	 * Update a specific message in the diracMessages array
	 * The entire operation (validate, update, save) is atomic to prevent races (RC-4)
	 */
	async updateDiracMessage(index: number, updates: Partial<DiracMessage>): Promise<void> {
		await this.withStateLock(async () => {
			if (index < 0 || index >= this.diracMessages.length) {
				throw new Error(`Invalid message index: ${index}`)
			}

			// Capture previous state before mutation
			const previousMessage = { ...this.diracMessages[index] }

			// Apply updates to the message
			Object.assign(this.diracMessages[index], updates)

			this.emitDiracMessagesChanged({
				type: "update",
				messages: this.diracMessages,
				index,
				previousMessage,
				message: this.diracMessages[index],
			})

			// Save changes
			this.debouncedSaveDiracMessages()
		})
		// History update is fire-and-forget; debouncedUpdateTaskHistory runs asynchronously
		this.debouncedUpdateTaskHistory()
	}

	/**
	 * Delete a specific message from the diracMessages array
	 * The entire operation (validate, delete, save) is atomic to prevent races (RC-4)
	 */
	async deleteDiracMessage(index: number): Promise<void> {
		await this.withStateLock(async () => {
			if (index < 0 || index >= this.diracMessages.length) {
				throw new Error(`Invalid message index: ${index}`)
			}

			// Capture the message before deletion
			const previousMessage = this.diracMessages[index]

			// Remove the message at the specified index
			this.diracMessages.splice(index, 1)

			this.emitDiracMessagesChanged({
				type: "delete",
				messages: this.diracMessages,
				index,
				previousMessage,
			})

			// Save changes
			this.debouncedSaveDiracMessages()
		})
		this.debouncedUpdateTaskHistory()
	}
}
