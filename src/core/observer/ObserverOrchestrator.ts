import { buildObserverConfig, type ObserverConfig, type ObservationEntry, type ObservationType } from "./ObserverConfig"
import { ObservationStore } from "./ObservationStore"
import { ObserverAgent } from "./ObserverAgent"
import { setObserverHealth } from "./index"
import { ReflectorAgent } from "./ReflectorAgent"
import type { StateManager } from "@core/storage/StateManager"
import type { DiracStorageMessage } from "@shared/messages/content"
import { Logger } from "@/shared/services/Logger"
import { AnalyzerClient } from "@/services/tree-sitter/AnalyzerClient"

export interface PrepareContextResult {
	messages: DiracStorageMessage[]
	observationBlock: string
	watcherInsights: string
	removedCount: number
}

interface ObserverCost {
    turnIndex: number
    type: ObservationType
    tokens: number
    latencyMs: number
}

export class ObserverCostTracker {
    private costs: ObserverCost[] = []

    add(type: ObservationType, tokens: number, latencyMs: number, turnIndex: number) {
        this.costs.push({ turnIndex, type, tokens, latencyMs })
        Logger.info("Observer", `[Cost] ${type} | tokens: ${tokens} | latency: ${latencyMs}ms | turn: ${turnIndex}`)
    }

    getSummary() {
        const totalTokens = this.costs.reduce((sum, c) => sum + c.tokens, 0)
        const totalLatency = this.costs.reduce((sum, c) => sum + c.latencyMs, 0)
        return {
            totalInvocations: this.costs.length,
            totalTokens,
            totalLatencyMs: totalLatency,
            avgLatencyMs: this.costs.length > 0 ? totalLatency / this.costs.length : 0
        }
    }
}

export class ObserverOrchestrator {
	private store: ObservationStore
	private agent: ObserverAgent | undefined
	private reflector: ReflectorAgent | undefined
    private costTracker = new ObserverCostTracker()
	private lastObservedMessageIndex = 0
	private pendingTasks = new Set<Promise<void>>()
	private _isEnabled: boolean
	private config: ObserverConfig
	consecutiveFailures = 0
	lastError: string | undefined

	constructor(
		private taskId: string,
		private stateManager: StateManager,
		private getAnalyzerClient?: () => AnalyzerClient | undefined,
	) {
		const settings = {
			observerEnabled: stateManager.getGlobalSettingsKey("observerEnabled"),
			observerProvider: stateManager.getGlobalSettingsKey("observerProvider"),
			observerModelId: stateManager.getGlobalSettingsKey("observerModelId"),
			observerTurns: stateManager.getGlobalSettingsKey("observerTurns"),
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
		const latest = this.store.getLatestObservation("summary")
		if (latest) {
			this.lastObservedMessageIndex = (latest.compressedRange?.[1] || 0) + 1
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

	async onTurnComplete(history: DiracStorageMessage[]): Promise<void> {
		if (!this._isEnabled) return

		const unobserved = this.getUnobservedMessages(history)
		
		// 1. WATCHER (Frequency controlled by observerTurns)
		const lastMsg = history[history.length - 1]
		const hasError = lastMsg?.content && JSON.stringify(lastMsg.content).includes("error")
		
		if (history.length % this.config.observerTurns === 0 || hasError) {
			this.triggerSpecializedObservation(history, "watcher")
		}

		// 2. RELEVANCE FILTER (Runs half as often as watcher, min every 5)
		const filterFreq = Math.max(5, this.config.observerTurns * 2)
		if (history.length % filterFreq === 0) {
			this.triggerSpecializedObservation(history, "filter")
		}

		// 3. SUMMARIZER (Context Compression)
		if (unobserved.length >= 4) {
			const tokenEstimate = this.estimateTokens(unobserved)
			const ratio = tokenEstimate / this.config.tokenThreshold

			if (this.config.blockAfter !== false && ratio >= this.config.blockAfter) {
				await this.runSummarizerSync(history, tokenEstimate)
			} else if (tokenEstimate >= this.config.tokenThreshold) {
				this.triggerSpecializedObservation(history, "summary", tokenEstimate)
			}
		}

		this.checkReflection()
	}

	private async runSummarizerSync(history: DiracStorageMessage[], tokenEstimate: number): Promise<void> {
		if (!this.agent) return
		const unobserved = this.getUnobservedMessages(history)
		if (unobserved.length === 0) return

		const sliceStart = Math.max(this.lastObservedMessageIndex, 2)
		const compressedRange: [number, number] = [sliceStart, history.length - 1]
        const startTime = Date.now()

		try {
			const observationText = await this.agent.observe(unobserved, "summary")
			if (!observationText) return

			const entry: ObservationEntry = {
				timestamp: Date.now(),
				type: "summary",
				observationText,
				compressedRange,
				tokenEstimate,
			}
			await this.store.append(entry)
			this.lastObservedMessageIndex = history.length
			this.consecutiveFailures = 0
			this.lastError = undefined
            
            const latency = Date.now() - startTime
            this.costTracker.add("summary", tokenEstimate, latency, history.length)

            // Index in C daemon if available
            const analyzer = this.getAnalyzerClient?.()
            if (analyzer) {
                await analyzer.indexObservation("summary", observationText, entry.timestamp, tokenEstimate)
            }

			Logger.debug(`[Observer] SYNC compressed ${unobserved.length} messages — block mode (ratio > ${this.config.blockAfter})`)
		} catch (error) {
			Logger.error("[Observer] Sync compression failed:", error)
			this.consecutiveFailures++
			this.lastError = error instanceof Error ? error.message : String(error)
			setObserverHealth(true, this.lastError)
		}
	}

	private triggerSpecializedObservation(history: DiracStorageMessage[], type: ObservationType, tokenEstimate?: number): void {
		if (!this.agent || this.pendingTasks.size > 2) return

		const unobserved = type === "summary" ? this.getUnobservedMessages(history) : history.slice(-10)
		if (unobserved.length === 0) return

		const sliceStart = Math.max(this.lastObservedMessageIndex, 2)
		const compressedRange: [number, number] | undefined = type === "summary" ? [sliceStart, history.length - 1] : undefined
        const startTime = Date.now()

		const promise = this.agent
			.observe(unobserved, type)
			.then(async (text) => {
				if (!text || text.includes("No alerts") || text.includes("Context clean")) return

				const entry: ObservationEntry = {
					timestamp: Date.now(),
					type,
					observationText: text,
					compressedRange,
					tokenEstimate: tokenEstimate || Math.ceil(text.length / 4),
				}

				await this.store.append(entry)
				if (type === "summary") this.lastObservedMessageIndex = history.length
				
				this.consecutiveFailures = 0
				this.lastError = undefined
				setObserverHealth(false)

                const latency = Date.now() - startTime
                this.costTracker.add(type, entry.tokenEstimate, latency, history.length)

                // Index in C daemon
                const analyzer = this.getAnalyzerClient?.()
                if (analyzer) {
                    await analyzer.indexObservation(type, text, entry.timestamp, entry.tokenEstimate)
                }

				Logger.debug(`[Observer] Finished ${type} observation (~${entry.tokenEstimate} tokens)`)
			})
			.catch((error) => {
				Logger.error(`[Observer] ${type} observation failed:`, error)
				this.consecutiveFailures++
				this.lastError = error instanceof Error ? error.message : String(error)
				setObserverHealth(true, this.lastError)
			})
			.finally(() => {
				this.pendingTasks.delete(promise)
			})
		
		this.pendingTasks.add(promise)
	}

	private checkReflection(): void {
		if (!this.config.reflectionEnabled || !this.reflector) return
		if (this.pendingTasks.size > 2) return

		const observationTokens = this.store.estimateTokenCount()
		if (observationTokens < this.config.reflectionTokenThreshold) return

		this.triggerReflection()
	}

	private triggerReflection(): void {
		if (!this.reflector) return

		const observationBlock = this.store.buildObservationBlock("summary")
		if (!observationBlock) return
        const startTime = Date.now()

		const promise = this.reflector
			.reflect(observationBlock)
			.then(async (reflectedText) => {
				if (!reflectedText) return

				const entry: ObservationEntry = {
					timestamp: Date.now(),
					type: "reflection",
					observationText: reflectedText,
					tokenEstimate: Math.ceil(reflectedText.length / 4),
				}

				await this.store.archiveAndReplace(entry)
                
                const latency = Date.now() - startTime
                this.costTracker.add("reflection", entry.tokenEstimate, latency, 0)

				Logger.debug(`[Observer] Reflected observation log`)
			})
			.catch((error) => {
				Logger.error("[Observer] Reflection failed:", error)
			})
			.finally(() => {
				this.pendingTasks.delete(promise)
			})
		
		this.pendingTasks.add(promise)
	}

	prepareContext(history: DiracStorageMessage[]): PrepareContextResult {
		if (!this._isEnabled) {
			return { messages: history, observationBlock: "", watcherInsights: "", removedCount: 0 }
		}

		const observationBlock = this.store.buildObservationBlock("summary")
		const watcherInsights = this.store.buildObservationBlock("watcher")
		const filterInsights = this.store.buildObservationBlock("filter")

		const combinedInsights = [
			watcherInsights,
			filterInsights
		].filter(Boolean).join("\n\n")

		if (observationBlock && this.lastObservedMessageIndex > 2) {
			const slicedMessages = [
				...history.slice(0, 2),
				...history.slice(this.lastObservedMessageIndex),
			]
			const removedCount = history.length - slicedMessages.length
			return { messages: slicedMessages, observationBlock, watcherInsights: combinedInsights, removedCount }
		}

		return { messages: history, observationBlock: "", watcherInsights: combinedInsights, removedCount: 0 }
	}

	/**
	 * Search archived and current observations using the C analyzer's FTS5 index.
	 */
	async recall(query: string): Promise<string> {
        const analyzer = this.getAnalyzerClient?.()
        if (analyzer) {
            const results = await analyzer.searchObservations(query)
            if (results.length > 0) {
                const lines = results.map((r, i) => {
                    const date = new Date(r.timestamp).toISOString().replace("T", " ").replace(/\.\d+Z$/, "")
                    return `${i + 1}. [${r.type.toUpperCase()}] [${date}] (~${r.tokens} tokens)\n${r.content}`
                })
                return `Found ${results.length} semantic matches in observation history:\n\n${lines.join("\n\n---\n\n")}`
            }
        }

        // Fallback to local keyword search if C daemon search returns nothing or is unavailable
		await this.store.load()
		const entries = this.store.getAllObservations()
		if (entries.length === 0) {
			return "No observations found."
		}

		const terms = query.toLowerCase().split(/\s+/).filter(Boolean)
		const matches = entries.filter((entry) => {
			const text = entry.observationText.toLowerCase()
			return terms.every((term) => text.includes(term))
		})

		if (matches.length === 0) {
			return `No observations matching "${query}".`
		}

		const lines = matches.map((entry, i) => {
			const date = new Date(entry.timestamp).toISOString().replace("T", " ").replace(/\.\d+Z$/, "")
			return `${i + 1}. [${entry.type.toUpperCase()}] [${date}] (~${entry.tokenEstimate} tokens)\n${entry.observationText}`
		})
		return `Found ${matches.length} keyword matches in observations:\n\n${lines.join("\n\n---\n\n")}`
	}

	async finalCompression(history: DiracStorageMessage[]): Promise<void> {
		if (!this._isEnabled || !this.agent) return
		const unobserved = this.getUnobservedMessages(history)
		if (unobserved.length < 2) return

		const sliceStart = Math.max(this.lastObservedMessageIndex, 2)
		const tokenEstimate = this.estimateTokens(unobserved)

		try {
			const observationText = await this.agent.observe(unobserved, "summary")
			if (!observationText) return

			const entry: ObservationEntry = {
				timestamp: Date.now(),
				type: "summary",
				observationText,
				compressedRange: [sliceStart, history.length - 1],
				tokenEstimate,
			}
			await this.store.append(entry)
		} catch (error) {
			Logger.error("[Observer] Final compression failed:", error)
		}
	}

	toggle(enabled: boolean): void {
		if (enabled && !this._isEnabled) {
			this._isEnabled = true
			this.config.enabled = true
			if (!this.agent) this.agent = new ObserverAgent(this.config, this.stateManager)
			if (this.config.reflectionEnabled && !this.reflector) this.reflector = new ReflectorAgent(this.config, this.stateManager)
		} else if (!enabled && this._isEnabled) {
			this._isEnabled = false
			this.config.enabled = false
			this.agent?.dispose()
			this.agent = undefined
			this.reflector = undefined
		}
	}

	async dispose(): Promise<void> {
		if (this.pendingTasks.size > 0) {
			const timeout = new Promise<void>((resolve) => setTimeout(resolve, 5000))
			await Promise.race([Promise.all(Array.from(this.pendingTasks)), timeout])
		}
		this.agent?.dispose()
		this.reflector?.dispose()
		await this.store.dispose()
	}
}
