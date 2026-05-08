import { buildObserverConfig, type ObserverConfig, type ObservationEntry, type ObservationType, type CriticAction, type SkeletonFidelity, LANGUAGE_NORMALIZATION } from "./ObserverConfig"
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
    success: boolean
    turn: number
    lang: string
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
 * ObserverOrchestrator - Manages the MEMENTO Memory Pyramid, SQS, and CPS.
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
    
    // Cognitive State
    private turnsSinceLastReflection = 0
    private actionHistory: ActionFeatures[] = []
    private lastSQS = 1.0
    private currentTier = 2
    private verboseLogs: string[] = []
    
    // Phase 6: Bandit Calibration
    private patternWeights: Map<string, number> = new Map()
    private eligibility: Map<string, number> = new Map()

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
        this.currentTier = this.detectTier(history)
	}

	private log(msg: string) {
        if (this.config.verbose) {
            this.verboseLogs.push(`[Observer] ${msg}`)
        }
        Logger.info("Observer", msg)
    }

    private detectTier(history: DiracStorageMessage[]): number {
        const fullText = JSON.stringify(history)
        if (fullText.includes("test") && (fullText.includes("pass") || fullText.includes("fail"))) return 0
        if (fullText.includes("lint") || fullText.includes("README.md")) return 1
        if (this.getAnalyzerClient?.()) return 2
        return 3
    }

    /**
     * Cross-Language AST-Churn Normalization (Müller et al. 2025).
     */
    private normalizedASTChurn(language: string, rawChurn: number, fileSize: number): number {
        const norm = LANGUAGE_NORMALIZATION[language] || LANGUAGE_NORMALIZATION.python
        const editNorm = rawChurn / norm.medianEditChurn
        const sizeNorm = Math.pow(fileSize / (norm.medianFileSize + 1), 0.3)
        return editNorm * sizeNorm
    }

    private async computeCPS(history: DiracStorageMessage[]): Promise<number> {
        const lastAction = this.extractActionFeatures(history.slice(-2)).pop()
        if (!lastAction) return 0.5

        const signals: number[] = []

        // 1. Syntactic Validity (0.25)
        signals.push(lastAction.success ? 1.0 : 0.0)

        // 2. AST Coverage (0.30)
        const filesTouched = new Set(history.slice(-10).filter(m => m.role === "assistant").map(m => {
             const c = JSON.stringify(m.content)
             return c.match(/path":\s*"([^"]+)"/)?.[1] || "global"
        }))
        signals.push(Math.min(filesTouched.size / 5, 1.0))

        // 3. Spec Alignment (0.25)
        signals.push(0.5)

        // 4. Command Outcome Entropy (0.20)
        const outcomes = history.slice(-6).filter(m => m.role === "tool").map(m => {
            const c = JSON.stringify(m.content)
            return c.includes("error") ? "FAIL" : "PASS"
        })
        const uniqueOutcomes = new Set(outcomes).size
        signals.push(uniqueOutcomes > 1 ? 0.8 : (outcomes[0] === "PASS" ? 0.6 : 0.1))

        return 0.25 * signals[0] + 0.30 * signals[1] + 0.25 * signals[2] + 0.20 * signals[3]
    }

    private async computeDCR(history: DiracStorageMessage[]): Promise<number> {
        const assistantMsgs = history.filter(m => m.role === "assistant").slice(-10)
        const filesTouched = new Set(assistantMsgs.map(m => {
             const c = JSON.stringify(m.content)
             return c.match(/path":\s*"([^"]+)"/)?.[1] || "global"
        }))
        
        const churn = await this.getLastASTChurn(history)
        const lastAction = this.extractActionFeatures(history.slice(-2)).pop()
        const normChurn = this.normalizedASTChurn(lastAction?.lang || "python", churn.added + churn.removed, churn.total)
        
        const coverage = filesTouched.size / 5
        return coverage * Math.min(normChurn / 2, 1.0)
    }

    private async computeSQS(history: DiracStorageMessage[]): Promise<{ sqs: number; status: string }> {
        const assistantMsgs = history.filter(m => m.role === "assistant").slice(-10)
        if (assistantMsgs.length === 0) return { sqs: 1.0, status: "FOCUSED" }

        const diffusion = 0.4 
        const eeRatio = this.computeEERatio(assistantMsgs)
        
        let sqs = 0
        if (this.currentTier === 0) {
            const testProgress = 0.5 
            sqs = 0.3 * (1 - diffusion) + 0.3 * eeRatio + 0.2 * (await this.computeDCR(history)) + 0.2 * testProgress
        } else {
            const cps = await this.computeCPS(history)
            const dcr = await this.computeDCR(history)
            sqs = 0.35 * (1 - diffusion) + 0.30 * eeRatio + 0.20 * dcr + 0.15 * cps
        }
        
        let status = "FOCUSED"
        const trigger = this.config.tierThresholds.sqs[this.currentTier]
        if (sqs < trigger) status = "STAGNATING"
        else if (sqs > 0.6) status = "EXPLORING"

        this.log(`Tier: ${this.currentTier} | SQS: ${sqs.toFixed(2)} | Status: ${status}`)
        return { sqs, status }
    }

    private computeEERatio(assistantMsgs: DiracStorageMessage[]): number {
        const actions = this.extractActionFeatures(assistantMsgs)
        const uniqueFiles = new Set(actions.map(a => a.file)).size
        let maxLoops = 1
        const counts: Record<string, number> = {}
        for (const a of actions) {
            const h = `${a.file}:${a.op}`; counts[h] = (counts[h] || 0) + 1
            maxLoops = Math.max(maxLoops, counts[h])
        }
        return (uniqueFiles / actions.length) * (1 / maxLoops)
    }

    private extractActionFeatures(msgs: DiracStorageMessage[]): ActionFeatures[] {
        return msgs.map((msg, i) => {
            const content = JSON.stringify(msg.content)
            const tool = content.match(/tool_code":\s*"([a-zA-Z0-9_]+)"/)?.[1] || "think"
            const file = content.match(/path":\s*"([^"]+)"/)?.[1] || "global"
            const lineMatch = content.match(/start_line":\s*([0-9]+)/)
            const success = !content.includes("error")
            const ext = file.split(".").pop() || "python"
            return {
                file,
                op: tool,
                lineRange: lineMatch ? lineMatch[1] : "0",
                errorSig: null,
                success,
                turn: i,
                lang: ext
            }
        })
    }

    private async getLastASTChurn(history: DiracStorageMessage[]): Promise<{ added: number; removed: number; total: number }> {
        const analyzer = this.getAnalyzerClient?.()
        if (!analyzer) return { added: 0, removed: 0, total: 0 }

        const lastAssistantMsg = history.filter(m => m.role === "assistant").pop()
        if (!lastAssistantMsg) return { added: 0, removed: 0, total: 0 }

        const content = JSON.stringify(lastAssistantMsg.content)
        const fileMatch = content.match(/path":\s*"([^"]+)"/)?.[1]
        const writeMatch = content.match(/new_content":\s*"([^"]+)"/)

        if (fileMatch && writeMatch) {
            return await analyzer.getASTChurn(fileMatch, "TODO_UNESCAPE")
        }
        return { added: 0, removed: 0, total: 0 }
    }

    private getDecayedConfidence(baseConf: number, type: "WATCHER" | "CRITIC", turnsSince: number): number {
        let tau = type === "WATCHER" ? this.config.tauWatcher : this.config.tauCritic
        if (this.lastSQS > 0.5) tau *= 2 // slow decay if progressing
        else tau *= 0.5 // fast decay if stagnating
        return baseConf * Math.exp(-turnsSince / tau)
    }

    /**
     * Implicit Signal Weighting (Park et al. 2026).
     */
    private pauseWeight(duration: number, lastHadError: boolean): number {
        let base = 0.02
        if (duration > 5) base *= 1.5
        if (duration > 12) base *= 2.5
        if (lastHadError) base *= 2.0
        return Math.min(base, 0.10)
    }

	async onTurnComplete(history: DiracStorageMessage[]): Promise<void> {
		if (!this._isEnabled) return
        this.turnsSinceLastReflection++
        this.currentTier = this.detectTier(history)

		const { sqs, status } = await this.computeSQS(history)
		this.lastSQS = sqs

        // 1. WATCHER (S1)
		if (history.length % this.config.observerTurns === 0 || status === "STAGNATING") {
			this.triggerSpecializedObservation(history, "watcher")
		}

		// 2. CRITIC (S2)
		if (history.length % this.config.criticFrequency === 0 && this.turnsSinceLastReflection >= this.config.reflectionCooldown) {
            const sqsTrigger = this.config.tierThresholds.sqs[this.currentTier]
            if (sqs < sqsTrigger + 0.1 || history.length % (this.config.criticFrequency * 2) === 0) {
			    this.triggerSpecializedObservation(history, "critic")
            }
		}

		// 3. SUMMARIZER / SKELETON
		const unobserved = this.getUnobservedMessages(history)
		if (unobserved.length >= 4) {
			const tokenEstimate = Math.ceil(JSON.stringify(unobserved).length / 4)
			const ratio = tokenEstimate / this.config.tokenThreshold

			if (this.config.blockAfter !== false && ratio >= this.config.blockAfter) {
				await this.runSummarizerSync(history, tokenEstimate)
			} else if (tokenEstimate >= this.config.tokenThreshold) {
				this.triggerSpecializedObservation(history, "skeleton", tokenEstimate)
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
                fidelity: "full"
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

		const unobserved = type === "summary" || type === "critic" || type === "skeleton" ? this.getUnobservedMessages(history) : history.slice(-12)
		if (unobserved.length === 0) return

		const sliceStart = Math.max(this.lastObservedMessageIndex, 2)
		const compressedRange: [number, number] | undefined = (type === "summary" || type === "skeleton") ? [sliceStart, history.length - 1] : undefined
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
                    sqs: this.lastSQS,
                    fidelity: type === "skeleton" ? "full" : undefined
				}

				await this.store.append(entry)
				if (type === "summary" || type === "skeleton") this.lastObservedMessageIndex = history.length
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
        const minConfidence = this.config.tierThresholds.confidence[this.currentTier]

        const filterWithDecay = (type: ObservationType) => {
            return this.store.getAllObservations()
                .filter(e => {
                    if (e.type !== type) return false
                    const turnsSince = (history.length - (e.compressedRange?.[1] || history.length))
                    const decayed = this.getDecayedConfidence(e.confidence ?? 1.0, type === "critic" ? "CRITIC" : "WATCHER", turnsSince)
                    return decayed >= minConfidence
                })
                .slice(-2)
                .map(e => e.observationText)
                .join("\n")
        }

		result.watcherInsights = [filterWithDecay("watcher"), filterWithDecay("filter"), filterWithDecay("critic")].filter(Boolean).join("\n")

        const latestCritic = this.store.getLatestObservation("critic")
        if (latestCritic && latestCritic.criticAction && latestCritic.criticAction !== "CONTINUE") {
            const turnsSince = (history.length - (latestCritic.compressedRange?.[1] || history.length))
            const decayed = this.getDecayedConfidence(latestCritic.confidence ?? 1.0, "CRITIC", turnsSince)
            if (decayed >= Math.min(0.7, minConfidence + 0.1)) {
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
        if (query === "--stats") return JSON.stringify(this.costTracker.getSummary())

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
