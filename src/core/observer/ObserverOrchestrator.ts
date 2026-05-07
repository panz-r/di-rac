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
    verboseLogs: string[]
}

interface ActionFeatures {
    file: string
    op: string
    lineRange: string
    errorSig: string | null
    turn: number
}

interface RetryState {
    count: number
    firstSeen: number
    sigs: string[]
}

export class ObserverCostTracker {
    private costs: Array<{ type: ObservationType; tokens: number; latencyMs: number; turn: number }> = []

    add(type: ObservationType, tokens: number, latencyMs: number, turn: number) {
        this.costs.push({ type, tokens, latencyMs, turn })
    }

    getSummary() {
        const totalTokens = this.costs.reduce((sum, c) => sum + c.tokens, 0)
        const totalLatency = this.costs.reduce((sum, c) => sum + c.latencyMs, 0)
        return {
            count: this.costs.length,
            totalTokens,
            totalLatencyMs: totalLatency,
            avgLatencyMs: this.costs.length > 0 ? totalLatency / this.costs.length : 0
        }
    }
}

/**
 * ObserverOrchestrator - Manages the MEMENTO Memory Pyramid and SQS.
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
    
    // Phase 4 State
    private turnsSinceLastReflection = 0
    private actionHistory: ActionFeatures[] = []
    private retryBuffer: Map<string, RetryState> = new Map()
    private lastSQS = 1.0
    private verboseLogs: string[] = []

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
            observerVerbose: stateManager.getGlobalSettingsKey("observerVerbose"),
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

	private log(msg: string) {
        if (this.config.verbose) {
            this.verboseLogs.push(`[Observer] ${msg}`)
        }
        Logger.info("Observer", msg)
    }

    /**
     * Compute Search Quality Score (SQS) - Zheng et al. 2026.
     * Combines Diffusion, E/E Ratio, and AST-Churn (DPR proxy).
     */
    private async computeSQS(history: DiracStorageMessage[]): Promise<{ sqs: number; status: string }> {
        const assistantMsgs = history.filter(m => m.role === "assistant").slice(-10)
        if (assistantMsgs.length === 0) return { sqs: 1.0, status: "FOCUSED" }

        // 1. E/E Ratio & Loop Detection (Wong et al. 2025)
        const recentActions = this.extractActionFeatures(assistantMsgs)
        const uniqueFiles = new Set(recentActions.map(a => a.file)).size
        const loopScore = this.detectLoops(recentActions)
        const eeRatio = (uniqueFiles / recentActions.length) * (1 / (loopScore + 0.1))

        // 2. Trajectory Diffusion (Zheng et al. 2026)
        // Simplified trigram similarity between last action and task
        const diffusion = 0.4 // Placeholder for actual calculation

        // 3. Depth-Progress Ratio (Zhang et al. 2025)
        // We call the Analyzer Daemon for AST-Churn
        const churn = await this.getLastASTChurn(history)
        const dpr = (churn.added + churn.removed) / (churn.total + 1)

        // 4. Combined SQS
        const sqs = 0.4 * (1 - diffusion) + 0.35 * eeRatio + 0.25 * Math.min(dpr * 10, 1.0)
        
        let status = "FOCUSED"
        if (sqs < 0.3) status = "STAGNATING"
        else if (sqs > 0.6) status = "EXPLORING"

        this.log(`SQS: ${sqs.toFixed(2)} | Status: ${status} | Churn: ${churn.added}/${churn.removed}`)
        return { sqs, status }
    }

    private extractActionFeatures(msgs: DiracStorageMessage[]): ActionFeatures[] {
        return msgs.map((msg, i) => {
            const content = JSON.stringify(msg.content)
            const tool = content.match(/tool_code":\s*"([a-zA-Z0-9_]+)"/)?.[1] || "think"
            const file = content.match(/path":\s*"([^"]+)"/)?.[1] || "global"
            const lineMatch = content.match(/start_line":\s*([0-9]+)/)
            return {
                file,
                op: tool,
                lineRange: lineMatch ? lineMatch[1] : "0",
                errorSig: null, // Populated later from tool result
                turn: i
            }
        })
    }

    private detectLoops(actions: ActionFeatures[]): number {
        const hashes = actions.map(a => `${a.file}:${a.op}`)
        let maxCount = 0
        const counts: Record<string, number> = {}
        for (const h of hashes) {
            counts[h] = (counts[h] || 0) + 1
            maxCount = Math.max(maxCount, counts[h])
        }
        return maxCount
    }

    private async getLastASTChurn(history: DiracStorageMessage[]): Promise<{ added: number; removed: number; total: number }> {
        const analyzer = this.getAnalyzerClient?.()
        if (!analyzer) return { added: 0, removed: 0, total: 0 }

        const lastAssistantMsg = history.filter(m => m.role === "assistant").pop()
        if (!lastAssistantMsg) return { added: 0, removed: 0, total: 0 }

        const content = JSON.stringify(lastAssistantMsg.content)
        const fileMatch = content.match(/path":\s*"([^"]+)"/)?.[1]
        const writeMatch = content.match(/new_content":\s*"([^"]+)"/) // Simplified match

        if (fileMatch && writeMatch) {
            // Note: Real implementation would need to properly unescape the content
            return await analyzer.getASTChurn(fileMatch, "TODO_UNESCAPE_CONTENT")
        }
        return { added: 0, removed: 0, total: 0 }
    }

    /**
     * Temporal Credit Assignment (Shen et al. 2025)
     */
    private getDecayedConfidence(baseConf: number, type: "WATCHER" | "CRITIC", turnsSince: number, progress: number): number {
        let tau = type === "WATCHER" ? this.config.tauWatcher : this.config.tauCritic
        if (progress > 0.1) tau *= 2
        if (progress < 0.05) tau *= 0.5
        return baseConf * Math.exp(-turnsSince / tau)
    }

	async onTurnComplete(history: DiracStorageMessage[]): Promise<void> {
		if (!this._isEnabled) return
        this.turnsSinceLastReflection++

		const { sqs, status } = await this.computeSQS(history)
		this.lastSQS = sqs

        // ADAPTIVE SCHEDULING (Singh et al. 2025)
		if (history.length % this.config.observerTurns === 0 || status === "STAGNATING") {
			this.triggerSpecializedObservation(history, "watcher")
		}

		if (history.length % this.config.criticFrequency === 0 && this.turnsSinceLastReflection >= this.config.reflectionCooldown) {
            if (sqs < 0.4 || history.length % (this.config.criticFrequency * 2) === 0) {
			    this.triggerSpecializedObservation(history, "critic")
            }
		}

		// HEAVY SUMMARIZER
		const unobserved = this.getUnobservedMessages(history)
		if (unobserved.length >= 4) {
			const tokenEstimate = Math.ceil(JSON.stringify(unobserved).length / 4)
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
            this.log(`SYNC compressed history (${tokenEstimate} tokens)`)

            const analyzer = this.getAnalyzerClient?.()
            if (analyzer) await analyzer.indexObservation("summary", observationText, entry.timestamp, tokenEstimate)
		} catch (error) {
			Logger.error("[Observer] Sync compression failed:", error)
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
                    criticAction,
                    sqs: this.lastSQS
				}

				await this.store.append(entry)
				if (type === "summary") this.lastObservedMessageIndex = history.length
                if (type === "reflection" || (type === "critic" && criticAction === "REFLECT")) {
                    this.turnsSinceLastReflection = 0
                }

                const latency = Date.now() - startTime
                this.costTracker.add(type, entry.tokenEstimate, latency, history.length)
                this.log(`Finished ${type} pass (conf: ${confidence.toFixed(2)})`)

                const analyzer = this.getAnalyzerClient?.()
                if (analyzer) {
                    if (type === "critic") await analyzer.indexCriticDecision(text, history.length, confidence)
                    else if (type === "watcher") await analyzer.indexWatcherPattern(text, "global", history.length)
                    else await analyzer.indexObservation(type, text, entry.timestamp, entry.tokenEstimate)
                }
			})
			.catch((error) => {
				Logger.error(`[Observer] ${type} failed:`, error)
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
				this.log("Reflected working context")
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
        const result: PrepareContextResult = { 
            messages: history, 
            observationBlock: "", 
            watcherInsights: "", 
            removedCount: 0,
            verboseLogs: [...this.verboseLogs]
        }
        this.verboseLogs = []

		if (!this._isEnabled) return result

		const observationBlock = this.store.buildObservationBlock("summary")
        
        const filterWithDecay = (type: ObservationType) => {
            return this.store.getAllObservations()
                .filter(e => {
                    if (e.type !== type) return false
                    const turnsSince = (history.length - (e.compressedRange?.[1] || history.length))
                    const decayed = this.getDecayedConfidence(e.confidence ?? 1.0, type === "critic" ? "CRITIC" : "WATCHER", turnsSince, 0.05)
                    return decayed >= this.config.confidenceThreshold
                })
                .slice(-2)
                .map(e => e.observationText)
                .join("\n")
        }

		const watcherInsights = filterWithDecay("watcher")
		const filterInsights = filterWithDecay("filter")
        const criticInsights = filterWithDecay("critic")

		result.watcherInsights = [watcherInsights, filterInsights, criticInsights].filter(Boolean).join("\n")

        const latestCritic = this.store.getLatestObservation("critic")
        if (latestCritic && latestCritic.criticAction && latestCritic.criticAction !== "CONTINUE") {
            const turnsSince = (history.length - (latestCritic.compressedRange?.[1] || history.length))
            const decayed = this.getDecayedConfidence(latestCritic.confidence ?? 1.0, "CRITIC", turnsSince, 0.05)
            if (decayed >= 0.7) {
                result.interruptReason = latestCritic.observationText
                result.criticAction = latestCritic.criticAction
            }
        }

		if (observationBlock && this.lastObservedMessageIndex > 2) {
			result.messages = [
				...history.slice(0, 2), 
				...history.slice(this.lastObservedMessageIndex),
			]
			result.removedCount = history.length - result.messages.length
            result.observationBlock = observationBlock
		}

		return result
	}

	async recall(query: string): Promise<string> {
        if (query === "--stats") return this.costTracker.formatSummary()

        const analyzer = this.getAnalyzerClient?.()
        if (analyzer) {
            const results = await analyzer.searchObservations(query)
            if (results.length > 0) {
                const lines = results.map((r, i) => {
                    const date = new Date(r.timestamp).toISOString().replace("T", " ").replace(/\.\d+Z$/, "")
                    return `${i + 1}. [${r.type.toUpperCase()}] [${date}]\n${r.content}`
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
			return `${i + 1}. [${entry.type.toUpperCase()}] [${date}]\n${entry.observationText}`
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
