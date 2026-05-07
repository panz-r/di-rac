import { buildObserverConfig, type ObserverConfig, type ObservationEntry, type ObservationType, type CriticAction } from "./ObserverConfig"
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
    interruptReason?: string
    criticAction?: CriticAction
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
        const count = this.costs.length
        return {
            count,
            totalTokens,
            totalLatencyMs: totalLatency,
            avgLatencyMs: count > 0 ? totalLatency / count : 0,
            avgTokens: count > 0 ? totalTokens / count : 0
        }
    }

    formatSummary(): string {
        const s = this.getSummary()
        return `Observer Session Stats: ${s.count} runs | ${s.totalTokens} tokens | total latency ${s.totalLatencyMs}ms | avg ${s.avgLatencyMs.toFixed(0)}ms/run`
    }
}

/**
 * ObserverOrchestrator - Manages the multi-layer cognitive stack.
 * Layer 2: Watcher (S1) - Fast, heuristic, every turn.
 * Layer 3: Critic (S2) - Slow, deliberative, adaptive.
 */
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
    consecutiveSuccesses = 0 // For adaptive scheduling
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
			observerCriticFrequency: stateManager.getGlobalSettingsKey("observerCriticFrequency"),
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

	/**
	 * Turn complete hook. Implements adaptive scheduling (Semi-MDP lite).
	 */
	async onTurnComplete(history: DiracStorageMessage[]): Promise<void> {
		if (!this._isEnabled) return

		const lastMsg = history[history.length - 1]
		const hasError = lastMsg?.content && JSON.stringify(lastMsg.content).includes("error")
		
        // ADAPTIVE SCHEDULING (System 1 control)
        if (hasError) {
            this.consecutiveSuccesses = 0
            // Run Watcher immediately on error
            this.triggerSpecializedObservation(history, "watcher")
        } else {
            this.consecutiveSuccesses++
            // Heuristic: if doing well, back off. If failing, run more often.
            const adaptiveFreq = this.consecutiveSuccesses > 3 ? this.config.observerTurns + 1 : this.config.observerTurns
            if (history.length % Math.min(adaptiveFreq, 10) === 0) {
                this.triggerSpecializedObservation(history, "watcher")
            }
        }

		// 2. RELEVANCE FILTER
		const filterFreq = Math.max(5, this.config.observerTurns * 3)
		if (history.length % filterFreq === 0) {
			this.triggerSpecializedObservation(history, "filter")
		}

		// 3. SLOW CRITIC (S2)
		if (history.length % this.config.criticFrequency === 0) {
			this.triggerSpecializedObservation(history, "critic")
		}

		// 4. HEAVY SUMMARIZER
		const unobserved = this.getUnobservedMessages(history)
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

            const analyzer = this.getAnalyzerClient?.()
            if (analyzer) {
                await analyzer.indexObservation("summary", observationText, entry.timestamp, tokenEstimate)
            }

			Logger.debug(`[Observer:S2] SYNC compressed ${unobserved.length} messages (headroom: ${(1.0 - (this.config.blockAfter as number)).toFixed(2)})`)
		} catch (error) {
			Logger.error("[Observer:S2] Sync compression failed:", error)
			this.consecutiveFailures++
			this.lastError = error instanceof Error ? error.message : String(error)
			setObserverHealth(true, this.lastError)
		}
	}

	private triggerSpecializedObservation(history: DiracStorageMessage[], type: ObservationType, tokenEstimate?: number): void {
		if (!this.agent || this.pendingTasks.size > 2) return

		const unobserved = type === "summary" || type === "critic" ? this.getUnobservedMessages(history) : history.slice(-10)
		if (unobserved.length === 0) return

		const sliceStart = Math.max(this.lastObservedMessageIndex, 2)
		const compressedRange: [number, number] | undefined = type === "summary" ? [sliceStart, history.length - 1] : undefined
        const startTime = Date.now()

		const promise = this.agent
			.observe(unobserved, type)
			.then(async (text) => {
				if (!text || text.includes("No alerts") || text.includes("Context clean")) return

                // Parse confidence and action
                let confidence = 1.0
                const confMatch = text.match(/confidence:([0-9.]+)/)
                if (confMatch) confidence = parseFloat(confMatch[1])

                // "Whisper, not shout": Filter low confidence insights from context injection
                // (They stay in the store for recall, but don't bother the actor)
                const isHighConfidence = confidence >= this.config.confidenceThreshold

                let criticAction: CriticAction | undefined
                if (type === "critic") {
                    const actionMatch = text.match(/action:([A-Z]+)/)
                    if (actionMatch) criticAction = actionMatch[1] as CriticAction
                }

				const entry: ObservationEntry = {
					timestamp: Date.now(),
					type,
					observationText: text,
					compressedRange,
					tokenEstimate: tokenEstimate || Math.ceil(text.length / 4),
                    confidence,
                    criticAction
				}

				await this.store.append(entry)
				if (type === "summary") this.lastObservedMessageIndex = history.length
				
				this.consecutiveFailures = 0
				this.lastError = undefined
				setObserverHealth(false)

                const latency = Date.now() - startTime
                this.costTracker.add(type, entry.tokenEstimate, latency, history.length)

                const analyzer = this.getAnalyzerClient?.()
                if (analyzer) {
                    await analyzer.indexObservation(type, text, entry.timestamp, entry.tokenEstimate)
                }

				Logger.debug(`[Observer:${type === "critic" ? "S2" : "S1"}] Finished ${type} observation (conf: ${confidence})`)
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

				Logger.debug(`[Observer:S2] Reflected observation log`)
			})
			.catch((error) => {
				Logger.error("[Observer:S2] Reflection failed:", error)
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
        
        // Filter insights by confidence threshold for context injection
        const filterHighConf = (type: ObservationType) => {
            return this.store.getAllObservations()
                .filter(e => e.type === type && (e.confidence ?? 1.0) >= this.config.confidenceThreshold)
                .slice(-2)
                .map(e => e.observationText)
                .join("\n")
        }

		const watcherInsights = filterHighConf("watcher")
		const filterInsights = filterHighConf("filter")
        const criticInsights = filterHighConf("critic")

		const combinedInsights = [
			watcherInsights,
			filterInsights,
            criticInsights
		].filter(Boolean).join("\n")

        const latestCritic = this.store.getLatestObservation("critic")
        let interruptReason: string | undefined
        let criticAction: CriticAction | undefined

        // Only act on high-confidence critic decisions
        if (latestCritic && latestCritic.criticAction && latestCritic.criticAction !== "CONTINUE" && (latestCritic.confidence ?? 1.0) >= 0.8) {
            interruptReason = latestCritic.observationText
            criticAction = latestCritic.criticAction
        }

		if (observationBlock && this.lastObservedMessageIndex > 2) {
			const slicedMessages = [
				...history.slice(0, 2),
				...history.slice(this.lastObservedMessageIndex),
			]
			const removedCount = history.length - slicedMessages.length
			return { 
                messages: slicedMessages, 
                observationBlock, 
                watcherInsights: combinedInsights, 
                removedCount,
                interruptReason,
                criticAction
            }
		}

		return { 
            messages: history, 
            observationBlock: "", 
            watcherInsights: combinedInsights, 
            removedCount: 0,
            interruptReason,
            criticAction
        }
	}

	async recall(query: string): Promise<string> {
        if (query === "--stats") return this.costTracker.formatSummary()

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

		await this.store.load()
		const entries = this.store.getAllObservations()
		if (entries.length === 0) return "No observations found."

		const terms = query.toLowerCase().split(/\s+/).filter(Boolean)
		const matches = entries.filter((entry) => {
			const text = entry.observationText.toLowerCase()
			return terms.every((term) => text.includes(term))
		})

		if (matches.length === 0) return `No observations matching "${query}".`

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
        Logger.info("Observer", this.costTracker.formatSummary())
		if (this.pendingTasks.size > 0) {
			const timeout = new Promise<void>((resolve) => setTimeout(resolve, 5000))
			await Promise.race([Promise.all(Array.from(this.pendingTasks)), timeout])
		}
		this.agent?.dispose()
		this.reflector?.dispose()
		await this.store.dispose()
	}
}
