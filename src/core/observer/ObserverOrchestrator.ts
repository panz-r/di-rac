import { buildObserverConfig, type ObserverConfig, type ObservationEntry } from "./ObserverConfig"
import { ObservationStore } from "./ObservationStore"
import { ObserverAgent } from "./ObserverAgent"
import { setObserverHealth } from "./index"
import { ReflectorAgent } from "./ReflectorAgent"
import type { StateManager } from "@core/storage/StateManager"
import type { DiracStorageMessage } from "@shared/messages/content"
import { Logger } from "@/shared/services/Logger"

export interface PrepareContextResult {
	messages: DiracStorageMessage[]
	observationBlock: string
	removedCount: number
}

export class ObserverOrchestrator {
	private store: ObservationStore
	private agent: ObserverAgent | undefined
	private reflector: ReflectorAgent | undefined
	private lastObservedMessageIndex = 0
	private pendingObserverPromise: Promise<void> | undefined
	private pendingReflectionPromise: Promise<void> | undefined
	private _isEnabled: boolean
	private config: ObserverConfig
	consecutiveFailures = 0
	lastError: string | undefined

	constructor(
		private taskId: string,
		private stateManager: StateManager,
	) {
		const settings = {
			observerEnabled: stateManager.getGlobalSettingsKey("observerEnabled"),
			observerProvider: stateManager.getGlobalSettingsKey("observerProvider"),
			observerModelId: stateManager.getGlobalSettingsKey("observerModelId"),
			observerTokenThreshold: stateManager.getGlobalSettingsKey("observerTokenThreshold"),
			observerBufferActivation: stateManager.getGlobalSettingsKey("observerBufferActivation"),
			observerBlockAfter: stateManager.getGlobalSettingsKey("observerBlockAfter"),
			observerReflectionEnabled: stateManager.getGlobalSettingsKey("observerReflectionEnabled"),
			observerReflectionTokenThreshold: stateManager.getGlobalSettingsKey("observerReflectionTokenThreshold"),
		}
		this.config = buildObserverConfig(settings)
		this._isEnabled = this.config.enabled
		this.store = new ObservationStore(taskId)
		if (this._isEnabled) {
			this.agent = new ObserverAgent(this.config, stateManager)
			if (this.config.reflectionEnabled) {
				this.reflector = new ReflectorAgent(this.config, stateManager)
			}
		}
	}

	get isEnabled(): boolean {
		return this._isEnabled
	}

	async initialize(history: DiracStorageMessage[]): Promise<void> {
		if (!this._isEnabled) return
		await this.store.load()
		const latest = this.store.getLatestObservation()
		if (latest) {
			this.lastObservedMessageIndex = latest.compressedRange[1] + 1
		} else {
			this.lastObservedMessageIndex = history.length
		}
	}

	private estimateTokens(messages: DiracStorageMessage[]): number {
		let totalChars = 0
		for (const msg of messages) {
			if (typeof msg.content === "string") {
				totalChars += msg.content.length
			} else if (Array.isArray(msg.content)) {
				for (const block of msg.content as any[]) {
					if ("text" in block && typeof block.text === "string") {
						totalChars += block.text.length
					}
				}
			}
		}
		return Math.ceil(totalChars / 4)
	}

	private getUnobservedMessages(history: DiracStorageMessage[]): DiracStorageMessage[] {
		const sliceStart = Math.max(this.lastObservedMessageIndex, 2)
		return history.slice(sliceStart)
	}

	/**
	 * Called after each turn. Implements three-mode catch-up:
	 * 1. Normal: async compression when tokens >= threshold
	 * 2. Buffer: halved threshold when at bufferActivation ratio
	 * 3. Block: synchronous when at blockAfter ratio
	 */
	async onTurnComplete(history: DiracStorageMessage[]): Promise<void> {
		if (!this._isEnabled) return
		if (this.pendingObserverPromise) return

		const unobserved = this.getUnobservedMessages(history)
		if (unobserved.length < 4) return

		const tokenEstimate = this.estimateTokens(unobserved)
		const ratio = tokenEstimate / this.config.tokenThreshold

		// Check reflection after each turn (cheap in-memory check)
		this.checkReflection()

		// Mode 3: Block mode — synchronous compression
		if (this.config.blockAfter !== false && ratio >= this.config.blockAfter) {
			await this.runCompressionSync(history, tokenEstimate)
			return
		}

		// Mode 1 or 2: Async compression with adjusted threshold
		let effectiveThreshold = this.config.tokenThreshold
		if (ratio >= this.config.bufferActivation) {
			// Buffer mode: halve the threshold for more frequent compression
			effectiveThreshold = Math.floor(this.config.tokenThreshold / 2)
		}

		if (tokenEstimate >= effectiveThreshold) {
			this.triggerCompression(history, tokenEstimate)
		}
	}

	private async runCompressionSync(history: DiracStorageMessage[], tokenEstimate: number): Promise<void> {
		if (!this.agent) return
		const unobserved = this.getUnobservedMessages(history)
		if (unobserved.length === 0) return

		const sliceStart = Math.max(this.lastObservedMessageIndex, 2)
		const compressedRange: [number, number] = [sliceStart, history.length - 1]

		try {
			const observationText = await this.agent.compress(unobserved)
			if (!observationText) return

			const entry: ObservationEntry = {
				timestamp: Date.now(),
				observationText,
				compressedRange,
				tokenEstimate,
			}
			await this.store.append(entry)
			this.lastObservedMessageIndex = history.length
			this.consecutiveFailures = 0
			this.lastError = undefined

			Logger.debug(
				`[Observer] SYNC compressed ${unobserved.length} messages (~${tokenEstimate} tokens) — block mode`,
			)
		} catch (error) {
			Logger.error("[Observer] Sync compression failed:", error)
			this.consecutiveFailures++
			this.lastError = error instanceof Error ? error.message : String(error)
			setObserverHealth(true, this.lastError)
		}
	}

	private triggerCompression(history: DiracStorageMessage[], tokenEstimate: number): void {
		if (!this.agent) return

		const unobserved = this.getUnobservedMessages(history)
		if (unobserved.length === 0) return

		const sliceStart = Math.max(this.lastObservedMessageIndex, 2)
		const compressedRange: [number, number] = [sliceStart, history.length - 1]

		this.pendingObserverPromise = this.agent
			.compress(unobserved)
			.then(async (observationText) => {
				if (!observationText) return

				const entry: ObservationEntry = {
					timestamp: Date.now(),
					observationText,
					compressedRange,
					tokenEstimate,
				}

				await this.store.append(entry)
				this.lastObservedMessageIndex = history.length
				this.consecutiveFailures = 0
				this.lastError = undefined
				setObserverHealth(false)

				Logger.debug(
					`[Observer] Compressed ${unobserved.length} messages (~${tokenEstimate} tokens) into observations`,
				)
			})
			.catch((error) => {
				Logger.error("[Observer] Compression failed:", error)
				this.consecutiveFailures++
				this.lastError = error instanceof Error ? error.message : String(error)
				setObserverHealth(true, this.lastError)
			})
			.finally(() => {
				this.pendingObserverPromise = undefined
			})
	}

	private checkReflection(): void {
		if (!this.config.reflectionEnabled || !this.reflector) return
		if (this.pendingReflectionPromise) return

		const observationTokens = this.store.estimateTokenCount()
		if (observationTokens < this.config.reflectionTokenThreshold) return

		this.triggerReflection()
	}

	private triggerReflection(): void {
		if (!this.reflector) return

		const observationBlock = this.store.buildObservationBlock()
		if (!observationBlock) return

		this.pendingReflectionPromise = this.reflector
			.reflect(observationBlock)
			.then(async (reflectedText) => {
				if (!reflectedText) return

				const entry: ObservationEntry = {
					timestamp: Date.now(),
					observationText: reflectedText,
					compressedRange: [0, 0], // Reflected entries don't map to message ranges
					tokenEstimate: Math.ceil(reflectedText.length / 4),
				}

				await this.store.archiveAndReplace(entry)

				Logger.debug(
					`[Observer] Reflected observation log (~${observationBlock.length / 4} tokens → ~${reflectedText.length / 4} tokens)`,
				)
			})
			.catch((error) => {
				Logger.error("[Observer] Reflection failed:", error)
			})
			.finally(() => {
				this.pendingReflectionPromise = undefined
			})
	}

	prepareContext(history: DiracStorageMessage[]): PrepareContextResult {
		if (!this._isEnabled) {
			return { messages: history, observationBlock: "", removedCount: 0 }
		}

		const observationBlock = this.store.buildObservationBlock()

		if (observationBlock && this.lastObservedMessageIndex > 2) {
			const slicedMessages = [
				...history.slice(0, 2),
				...history.slice(this.lastObservedMessageIndex),
			]
			const removedCount = history.length - slicedMessages.length
			return { messages: slicedMessages, observationBlock, removedCount }
		}

		return { messages: history, observationBlock, removedCount: 0 }
	}

	/**
	 * Search archived and current observations by keyword.
	 * Used by the dirac_recall tool.
	 */
	async recall(query: string): Promise<string> {
		await this.store.load()
		const entries = this.store.getAllObservations()
		if (entries.length === 0) {
			return "No observations found. Observations are generated when the observer is enabled and compresses conversation history."
		}

		const terms = query.toLowerCase().split(/\s+/).filter(Boolean)
		const matches = entries.filter((entry) => {
			const text = entry.observationText.toLowerCase()
			return terms.every((term) => text.includes(term))
		})

		if (matches.length === 0) {
			return `No observations matching "${query}". Try broader keywords or use dirac_recall without arguments to list all.`
		}

		const lines = matches.map((entry, i) => {
			const date = new Date(entry.timestamp).toISOString().replace("T", " ").replace(/\.\d+Z$/, "")
			return `${i + 1}. [${date}] (~${entry.tokenEstimate} tokens)\n${entry.observationText}`
		})
		return `Found ${matches.length} observation${matches.length > 1 ? "s" : ""} matching "${query}":\n\n${lines.join("\n\n---\n\n")}`
	}

	/**
	 * Run a final compression pass (called on session end).
	 */
	async finalCompression(history: DiracStorageMessage[]): Promise<void> {
		if (!this._isEnabled || !this.agent) return

		const unobserved = this.getUnobservedMessages(history)
		if (unobserved.length < 2) return

		const sliceStart = Math.max(this.lastObservedMessageIndex, 2)
		const tokenEstimate = this.estimateTokens(unobserved)

		try {
			const observationText = await this.agent.compress(unobserved)
			if (!observationText) return

			const entry: ObservationEntry = {
				timestamp: Date.now(),
				observationText,
				compressedRange: [sliceStart, history.length - 1],
				tokenEstimate,
			}
			await this.store.append(entry)
			Logger.debug(`[Observer] Final compression: ${unobserved.length} messages captured`)
		} catch (error) {
			Logger.error("[Observer] Final compression failed:", error)
		}
	}

	toggle(enabled: boolean): void {
		if (enabled && !this._isEnabled) {
			this._isEnabled = true
			this.config.enabled = true
			if (!this.agent) {
				this.agent = new ObserverAgent(this.config, this.stateManager)
			}
			if (this.config.reflectionEnabled && !this.reflector) {
				this.reflector = new ReflectorAgent(this.config, this.stateManager)
			}
		} else if (!enabled && this._isEnabled) {
			this._isEnabled = false
			this.config.enabled = false
			this.agent?.dispose()
			this.agent = undefined
			this.reflector?.dispose()
			this.reflector = undefined
		}
	}

	async dispose(): Promise<void> {
		const pending = [this.pendingObserverPromise, this.pendingReflectionPromise].filter(Boolean)
		if (pending.length > 0) {
			const timeout = new Promise<void>((resolve) => setTimeout(resolve, 5000))
			await Promise.race([Promise.all(pending), timeout])
		}
		this.agent?.dispose()
		this.reflector?.dispose()
		await this.store.dispose()
	}
}
