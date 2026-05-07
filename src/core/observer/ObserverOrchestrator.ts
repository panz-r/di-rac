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

interface ActionFeatures {
    file: string
    op: string
    errorSig: string
}

export class ObserverCostTracker {
    private costs: Array<{ type: ObservationType; tokens: number; latencyMs: number; turn: number }> = []

    add(type: ObservationType, tokens: number, latencyMs: number, turn: number) {
        this.costs.push({ type, tokens, latencyMs, turn })
    }

    formatSummary(): string {
        const totalTokens = this.costs.reduce((sum, c) => sum + c.tokens, 0)
        const totalLatency = this.costs.reduce((sum, c) => sum + c.latencyMs, 0)
        return `Observer Session Stats: ${this.costs.length} runs | ${totalTokens} tokens | total latency ${totalLatency}ms`
    }
}

/**
 * ObserverOrchestrator - Manages the MEMENTO Memory Pyramid (Wang et al. 2025).
 * Layer 0: Pinned Recency (S2)
 * Layer 1: Structured Skeleton (S2)
 * Layer 2: Decision Rationale (S2)
 * Layer 3: Compressed Patterns (S1)
 * Layer 4: Deep Log (S1)
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
    
    // Trajectory Tracking
    private turnsSinceLastReflection = 0
    private actionLog: ActionFeatures[] = []
    private loopHashes: Map<string, number> = new Map()

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
			this.lastObservedMessageIndex = Math.max(0, history.length - 10)
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
     * Search Quality Score (SQS) - Zheng et al. 2026.
     * α × (1-diffusion) + β × E/E_ratio + γ × DPR
     */
    private computeSQS(history: DiracStorageMessage[]): { sqs: number; status: string } {
        const assistantMsgs = history.filter(m => m.role === "assistant").slice(-10)
        if (assistantMsgs.length === 0) return { sqs: 1.0, status: "FOCUSED" }

        // 1. E/E Ratio (Unique files / Loop score)
        const currentActions = assistantMsgs.map(msg => {
            const content = JSON.stringify(msg.content)
            const tool = content.match(/tool_code":\s*"([a-zA-Z0-9_]+)"/)?.[1] || "think"
            const file = content.match(/path":\s*"([^"]+)"/)?.[1] || "global"
            return { file, tool }
        })

        const uniqueFiles = new Set(currentActions.map(a => a.file)).size
        const loopHash = (a: any) => `${a.file}:${a.tool}`
        let maxLoops = 1
        const counts: Record<string, number> = {}
        for (const a of currentActions) {
            const h = loopHash(a); counts[h] = (counts[h] || 0) + 1
            maxLoops = Math.max(maxLoops, counts[h])
        }
        const eeRatio = (uniqueFiles / currentActions.length) * (1 / maxLoops)

        // 2. Trajectory Diffusion (Simplified Trigram Similarity to task)
        // Note: Real implementation needs embeddings.
        const diffusion = 0.5 // Placeholder for now

        // 3. Combined SQS
        const sqs = 0.6 * eeRatio + 0.4 * (1 - diffusion)
        
        let status = "FOCUSED"
        if (sqs < 0.3) status = "STAGNATING"
        else if (sqs > 0.6) status = "EXPLORING"

        return { sqs, status }
    }

	async onTurnComplete(history: DiracStorageMessage[]): Promise<void> {
		if (!this._isEnabled) return
        this.turnsSinceLastReflection++

		const { sqs, status } = this.computeSQS(history)
		
        // 1. WATCHER (S1) - Heuristic Trigger
		if (history.length % this.config.observerTurns === 0 || status === "STAGNATING") {
			this.triggerSpecializedObservation(history, "watcher")
		}

		// 2. CRITIC (S2) - Gated by Cooldown
		if (history.length % this.config.criticFrequency === 0 && this.turnsSinceLastReflection >= this.config.reflectionCooldown) {
			this.triggerSpecializedObservation(history, "critic")
		}

		// 3. SUMMARIZER (Context Compression)
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
            
            const latency = Date.now() - startTime
            this.costTracker.add("summary", tokenEstimate, latency, history.length)

            const analyzer = this.getAnalyzerClient?.()
            if (analyzer) {
                await analyzer.indexObservation("summary", observationText, entry.timestamp, tokenEstimate)
            }

			Logger.debug(`[Observer:S2] SYNC compressed history (ratio: ${(tokenEstimate / this.config.tokenThreshold).toFixed(2)})`)
		} catch (error) {
			Logger.error("[Observer:S2] Sync compression failed:", error)
		}
	}

	private triggerSpecializedObservation(history: DiracStorageMessage[], type: ObservationType, tokenEstimate?: number): void {
		if (!this.agent || this.pendingTasks.size > 3) return

		const unobserved = type === "summary" || type === "critic" ? this.getUnobservedMessages(history) : history.slice(-12)
		if (unobserved.length === 0) return

		const sliceStart = Math.max(this.lastObservedMessageIndex, 2)
		const compressedRange: [number, number] | undefined = type === "summary" ? [sliceStart, history.length - 1] : undefined
        const startTime = Date.now()

		const promise = this.agent
			.observe(unobserved, type)
			.then(async (text) => {
				if (!text || text.includes("No alerts") || text.includes("Context clean")) return

                let confidence = 1.0
                const confMatch = text.match(/confidence:([0-9.]+)/)
                if (confMatch) confidence = parseFloat(confMatch[1])

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
                if (type === "reflection" || (type === "critic" && criticAction === "REFLECT")) {
                    this.turnsSinceLastReflection = 0
                }

                const latency = Date.now() - startTime
                this.costTracker.add(type, entry.tokenEstimate, latency, history.length)

                const analyzer = this.getAnalyzerClient?.()
                if (analyzer) {
                    if (type === "critic") await analyzer.indexCriticDecision(text, history.length, confidence)
                    else if (type === "watcher") await analyzer.indexWatcherPattern(text, "global", history.length)
                    else await analyzer.indexObservation(type, text, entry.timestamp, entry.tokenEstimate)
                }

				Logger.debug(`[Observer:${type === "critic" ? "S2" : "S1"}] Finished ${type} (conf: ${confidence})`)
			})
			.catch((error) => {
				Logger.error(`[Observer] ${type} observation failed:`, error)
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
                this.turnsSinceLastReflection = 0
                
                const latency = Date.now() - startTime
                this.costTracker.add("reflection", entry.tokenEstimate, latency, 0)
				Logger.debug(`[Observer:S2] Reflected working context`)
			})
			.catch((error) => {
				Logger.error("[Observer:S2] Reflection failed:", error)
			})
			.finally(() => {
				this.pendingTasks.delete(promise)
			})
		
		this.pendingTasks.add(promise)
	}

    /**
     * MEMENTO Memory Pyramid implementation.
     */
	prepareContext(history: DiracStorageMessage[]): PrepareContextResult {
		if (!this._isEnabled) {
			return { messages: history, observationBlock: "", watcherInsights: "", removedCount: 0 }
		}

		const observationBlock = this.store.buildObservationBlock("summary")
        
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
        if (latestCritic && latestCritic.criticAction && latestCritic.criticAction !== "CONTINUE" && (latestCritic.confidence ?? 1.0) >= 0.75) {
            interruptReason = latestCritic.observationText
            criticAction = latestCritic.criticAction
        }

		if (observationBlock && this.lastObservedMessageIndex > 2) {
            // MEMENTO: S2 gets Layer 0 (last 4K tokens) + Layer 1 (Working Set summary)
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
                return `Found ${results.length} semantic matches:\n\n${lines.join("\n\n---\n\n")}`
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
		return `Found ${matches.length} keyword matches:\n\n${lines.join("\n\n---\n\n")}`
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
