import { Anthropic } from "@anthropic-ai/sdk"
import { AssistantMessageContent } from "@core/assistant-message"
import { DiracAskResponse } from "@shared/WebviewMessage"
import type { HookExecution } from "./types/HookExecution"
import { SkillMetadata } from "@/shared/skills"

export class TaskState {
	// Task-level timing
	taskStartTimeMs = Date.now()
	taskFirstTokenTimeMs?: number

	// Streaming flags
	isStreaming = false
	isWaitingForFirstChunk = false
	didCompleteReadingStream = false

	// Content processing
	currentStreamingContentIndex = 0
	assistantMessageContent: AssistantMessageContent[] = []
	useNativeToolCalls = false
	userMessageContent: (Anthropic.TextBlockParam | Anthropic.ImageBlockParam | Anthropic.ToolResultBlockParam)[] = []
	userMessageContentReady = false
	// Map of tool names to their tool_use_id for creating proper ToolResultBlockParam
	toolUseIdMap: Map<string, string> = new Map()

	// Presentation locks
	presentAssistantMessageLocked = false
	presentAssistantMessageHasPendingUpdates = false

	// Ask/Response handling
	askResponse?: DiracAskResponse
	askResponseText?: string
	askResponseImages?: string[]
	askResponseFiles?: string[]
	lastMessageTs?: number

	// Plan mode specific state
	isAwaitingPlanResponse = false
	didRespondToPlanAskBySwitchingMode = false
	didSwitchToActMode = false

	// Context and history
	conversationHistoryDeletedRange?: [number, number]

	// Tool execution flags
	didRejectTool = false
	didAlreadyUseTool = false
	didEditFile = false

	// Error tracking
	consecutiveMistakeCount = 0
	doubleCheckCompletionPending = false
	didAutomaticallyRetryFailedApiRequest = false

	// Retry tracking for auto-retry feature
	autoRetryAttempts = 0

	// Task Initialization
	isInitialized = false


	// Task Abort / Cancellation
	abort = false
	didFinishAbortingStream = false
	abandoned = false

	// Hook execution tracking for cancellation
	activeHookExecution?: HookExecution

	// Auto-context summarization
	currentlySummarizing = false
	totalToolCallCount = 0

	lastAutoCompactTriggerIndex?: number
	taskLockAcquired = false
	availableSkills: SkillMetadata[] = []
	discoveredSkillsCache?: SkillMetadata[]

	// Exploration state
	fileCursors: Map<string, number> = new Map() // maps absolute path to start line (1-based)
	symbolIndex: Map<string, SymbolIndexEntry[]> = new Map() // maps relative path to symbols
	readCounts: Map<string, number> = new Map() // maps absolute path to number of times read
	contentHashCache: Map<string, string> = new Map() // maps cache key to content hash

	// Queued user messages (injected during streaming, drained at next turn)
	queuedUserMessages: Array<{ text: string; images?: string[] }> = []

	// Deferred user messages (injected after task completion, one per completion)
	deferredUserMessages: Array<{ text: string; images?: string[] }> = []

	// Advanced recovery state
	fileLastAccessToolIndex: Map<string, number> = new Map() // maps absolute path to tool call index
	filesTouchedInCurrentTurn: Set<string> = new Set()
	filesEditedInCurrentTurn: Set<string> = new Set()
	symbolIndexMtimes: Map<string, number> = new Map() // maps absolute path to last indexed mtime
	currentTaskPhase: "exploration" | "editing" | "verification" = "exploration"
	turnTokenEstimates: number = 0
	didExecuteCommand: boolean = false
	currentTurnNumber = 0

	// Observer state
	observerLastObservedIndex = 0
	observerUnobservedTokenEstimate = 0

	// Round 2 safeguard state
	editStreakCount: Map<string, number> = new Map()
	sessionLinesAdded = 0
	sessionLinesDeleted = 0

	// Round 3 proof-of-execution state
	bashExecutionHistory: Array<{
		executionId: string
		command: string
		exitCode: number | null
		timestamp: number
	}> = []
	bashExecutionCounter: number = 0
	lastEditTimestamp: number = 0

	// Round 3 turn snapshot state
	previousTurnFiles: Set<string> = new Set()

	// Round 4 output truncation state
	lastResponseWasTruncated: boolean = false
	truncatedOutputTokens: number = 0

	// Round 4 strengthened completion gate
	verificationTrivialCount: number = 0

	resetTurnState(): void {
		this.previousTurnFiles = new Set(this.filesEditedInCurrentTurn)
		this.lastResponseWasTruncated = false
		this.truncatedOutputTokens = 0
		this.filesTouchedInCurrentTurn.clear()
		this.filesEditedInCurrentTurn.clear()
		this.turnTokenEstimates = 0
		this.currentTurnNumber++
		this.queuedUserMessages = []
	}

	drainQueuedUserMessages(): Array<{ text: string; images?: string[] }> {
		const messages = this.queuedUserMessages.splice(0)
		return messages
	}

	clearQueuedUserMessages(): void {
		this.queuedUserMessages = []
	}

	drainDeferredMessage(): { text: string; images?: string[] } | undefined {
		return this.deferredUserMessages.shift()
	}

	clearDeferredUserMessages(): void {
		this.deferredUserMessages = []
	}
}

export interface SymbolIndexEntry {
	id: string
	name: string
	kind: string
	line: number
	signature?: string
}
