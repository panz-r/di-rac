import { ChildProcess, spawn } from "child_process"
import * as path from "path"
import * as fs from "fs"
import { Logger } from "@/shared/services/Logger"
import type { ParsedDefinition } from "./index"

export interface DaemonSymbol {
	name: string
	kind: string
	handle: string
	start_line: number
	end_line: number
	signature?: string
	parent?: string
}

export interface DaemonExpandResult {
	body: string
	start_line: number
	end_line: number
}

export interface DaemonOutlineResult {
	symbols: DaemonSymbol[]
	imports: string[]
}

export interface SyntaxError {
	start_line: number
	start_col: number
	end_line: number
	end_col: number
	message: string
	is_missing: boolean
}

export interface CheckSyntaxResult {
	has_errors: boolean
	errors: SyntaxError[]
}

export interface DaemonSearchResult {
	name: string
	kind: string
	type?: string  // "d", "r", "a", "i" — from persistent index
	handle: string
	file: string
	start_line: number
	start_col?: number
	end_line: number
	end_col?: number
	parent?: string
	signature?: string
}

interface PendingRequest {
	resolve: (value: any) => void
	reject: (reason: any) => void
	timer: ReturnType<typeof setTimeout>
	startTime: number
}

export interface DaemonMetrics {
	requestCount: number
	failedRequestCount: number
	totalLatencyMs: number
	restartCount: number
	uptimeMs: number
	lastCrashTime?: number
	lastSuccessfulRequestTime?: number
	isRunning: boolean
	consecutiveFailures: number
}

export interface DaemonHealthStatus {
	healthy: boolean
	reason?: string
	metrics: DaemonMetrics
}

export class AnalyzerClient {
	private process: ChildProcess | null = null
	private requestId = 0
	private pending = new Map<number, PendingRequest>()
	private buffer = ""
	private binaryPath: string
	private workspaceRoot: string
	private crashed = false
	private restartTimer: ReturnType<typeof setTimeout> | null = null
	private shuttingDown = false

	/** Set to true if the daemon cannot be started (binary missing, etc.) */
	fallback = true

	// Callers waiting for daemon to become ready after a crash/restart
	private readyResolvers: Array<() => void> = []

	// Metrics
	private startTime: number = 0
	private requestCount: number = 0
	private failedRequestCount: number = 0
	private totalLatencyMs: number = 0
	private restartCount: number = 0
	private lastCrashTime: number = 0
	private lastSuccessfulRequestTime: number = 0
	private consecutiveFailures: number = 0

	// Resilience settings — no cap on restarts for long-running sessions
	private readonly baseRestartDelayMs = 2000
	private readonly maxRestartDelayMs = 60000

	constructor(binaryPath: string, workspaceRoot: string) {
		this.binaryPath = binaryPath
		this.workspaceRoot = workspaceRoot
	}

	async start(): Promise<void> {
		if (this.shuttingDown) return
		if (this.process && !this.process.killed && !this.crashed) return
		if (!fs.existsSync(this.binaryPath)) {
			throw new Error(`Analyzer binary not found: ${this.binaryPath}. Cannot start without it.`)
		}
		try {
			await this.spawnDaemon()
			this.fallback = false
			this.startTime = Date.now()
			Logger.info("AnalyzerClient", "Daemon started")
		} catch (e) {
			throw new Error(`Analyzer daemon failed to start: ${e}`)
		}
	}

	private spawnDaemon(): Promise<void> {
		return new Promise((resolve, reject) => {
			const proc = spawn(this.binaryPath, ["--workspace-root", this.workspaceRoot], {
				stdio: ["pipe", "pipe", "pipe"],
			})

			let startupResolved = false

			proc.stdout!.on("data", (chunk: Buffer) => {
				this.handleData(chunk)
				if (!startupResolved) {
					startupResolved = true
					resolve()
				}
			})

			proc.stderr!.on("data", (chunk: Buffer) => {
				const msg = chunk.toString().trim()
				if (msg.includes("ready")) {
					if (!startupResolved) {
						startupResolved = true
						resolve()
					}
				}
			})

			proc.on("error", (err) => {
				Logger.warn("AnalyzerClient", `Daemon error: ${err.message}`)
				if (!startupResolved) {
					startupResolved = true
					reject(err)
				}
				this.handleCrash()
			})

			proc.on("exit", (code) => {
				Logger.info("AnalyzerClient", `Daemon exited with code ${code}`)
				if (!startupResolved) {
					startupResolved = true
					reject(new Error(`Daemon exited during startup with code ${code}`))
				}
				this.handleCrash()
			})

			this.process = proc
		})
	}

	private handleData(chunk: Buffer): void {
		this.buffer += chunk.toString()
		const lines = this.buffer.split("\n")
		this.buffer = lines.pop()! // keep incomplete last line

		for (const line of lines) {
			const trimmed = line.trim()
			if (!trimmed) continue
			try {
				const response = JSON.parse(trimmed)
				const id = response.id
				if (typeof id === "number" && this.pending.has(id)) {
					const pending = this.pending.get(id)!
					this.pending.delete(id)
					clearTimeout(pending.timer)
					const latency = Date.now() - pending.startTime
					this.totalLatencyMs += latency
					if (response.ok === false) {
						pending.reject(response.error || response)
					} else {
						this.lastSuccessfulRequestTime = Date.now()
						this.consecutiveFailures = 0
						pending.resolve(response)
					}
				}
			} catch {
				// Not a JSON line we care about
			}
		}
	}

	private handleCrash(): void {
		this.process = null
		// Reject all pending requests
		for (const [id, pending] of this.pending) {
			clearTimeout(pending.timer)
			const latency = Date.now() - pending.startTime
			this.totalLatencyMs += latency
			this.failedRequestCount++
			pending.reject(new Error("Daemon crashed"))
		}
		this.pending.clear()
		this.crashed = true
		this.lastCrashTime = Date.now()
		this.consecutiveFailures++

		if (!this.shuttingDown) {
			// Exponential backoff (no cap on attempts — long-running sessions need resilience)
			const delay = Math.min(
				this.baseRestartDelayMs * Math.pow(2, Math.min(this.restartCount, 6)),
				this.maxRestartDelayMs
			)
			Logger.info("AnalyzerClient", `Scheduling daemon restart in ${delay}ms`)
			this.restartTimer = setTimeout(() => {
				this.restart()
			}, delay)
		}
	}

	private async restart(): Promise<void> {
		if (this.shuttingDown) return
		this.restartCount++
		Logger.info("AnalyzerClient", `Restarting daemon (attempt ${this.restartCount})...`)
		try {
			await this.spawnDaemon()
			this.crashed = false
			this.fallback = false
			this.startTime = Date.now()
			Logger.info("AnalyzerClient", "Daemon restarted")

			// Notify any callers waiting for the daemon to become ready
			const resolvers = this.readyResolvers
			this.readyResolvers = []
			for (const resolve of resolvers) resolve()
		} catch (e) {
			Logger.warn("AnalyzerClient", `Restart failed: ${e}`)
			// Continue retrying via handleCrash
			this.handleCrash()
		}
	}

	async shutdown(): Promise<void> {
		this.shuttingDown = true
		if (this.restartTimer) {
			clearTimeout(this.restartTimer)
			this.restartTimer = null
		}
		if (this.process && !this.process.killed) {
			try {
				await this.send({ command: "shutdown" }, 3000)
			} catch {
				// Daemon may have already exited
			}
			this.process.kill()
			this.process = null
		}
		this.fallback = true
	}

	private async waitForReady(timeoutMs: number): Promise<void> {
		if (this.process && !this.process.killed && !this.crashed) return
		if (this.shuttingDown) throw new Error("Daemon shut down")

		return new Promise<void>((resolve, reject) => {
			const timer = setTimeout(() => {
				this.readyResolvers = this.readyResolvers.filter(r => r !== onReady)
				reject(new Error("Timed out waiting for daemon to become ready"))
			}, timeoutMs)
			const onReady = () => {
				clearTimeout(timer)
				resolve()
			}
			this.readyResolvers.push(onReady)
		})
	}

	private send(payload: Record<string, unknown>, timeoutMs = 60000): Promise<any> {
		return new Promise((resolve, reject) => {
			if (this.shuttingDown) {
				reject(new Error("Daemon shut down"))
				return
			}
			if (this.fallback && (!this.process || this.process.killed)) {
				// Daemon never started - attempt recovery
				this.restart().then(() => {
					this.doSend(payload, timeoutMs).then(resolve, reject)
				}).catch(() => {
					reject(new Error("Analyzer daemon unavailable"))
				})
				return
			}
			if (!this.process || this.process.killed || this.crashed) {
				// Daemon is restarting — wait for it to become ready
				this.waitForReady(timeoutMs).then(() => {
					this.doSend(payload, timeoutMs).then(resolve, reject)
				}).catch(reject)
				return
			}
			this.doSend(payload, timeoutMs).then(resolve, reject)
		})
	}

	private doSend(payload: Record<string, unknown>, timeoutMs: number): Promise<any> {
		return new Promise((resolve, reject) => {
			if (!this.process || this.process.killed) {
				reject(new Error("Daemon not running"))
				return
			}
			const id = ++this.requestId
			const message = JSON.stringify({ id, ...payload }) + "\n"
			const startTime = Date.now()
			const timer = setTimeout(() => {
				this.pending.delete(id)
				this.totalLatencyMs += Date.now() - startTime
				this.failedRequestCount++
				reject(new Error(`Request ${id} timed out after ${timeoutMs}ms`))
			}, timeoutMs)
			this.pending.set(id, { resolve, reject, timer, startTime })
			this.requestCount++
			this.process.stdin!.write(message)
		})
	}

	/**
	 * Health check for the daemon
	 * Returns detailed status including metrics
	 */
	async health(): Promise<DaemonHealthStatus> {
		const metrics = this.getMetrics()

		if (this.fallback && !this.process) {
			return {
				healthy: false,
				reason: "Daemon not running",
				metrics,
			}
		}

		if (this.crashed) {
			return {
				healthy: false,
				reason: "Daemon crashed, restart pending",
				metrics,
			}
		}

		// Try a ping to verify the daemon is responsive
		try {
			await this.ping()
			return {
				healthy: true,
				metrics,
			}
		} catch {
			return {
				healthy: false,
				reason: "Daemon not responsive to ping",
				metrics,
			}
		}
	}

	/**
	 * Ping/pong for liveness probes
	 */
	async ping(): Promise<void> {
		await this.send({ command: "status" }, 5000)
	}

	/**
	 * Get current metrics
	 */
	getMetrics(): DaemonMetrics {
		return {
			requestCount: this.requestCount,
			failedRequestCount: this.failedRequestCount,
			totalLatencyMs: this.totalLatencyMs,
			restartCount: this.restartCount,
			uptimeMs: this.startTime > 0 ? Date.now() - this.startTime : 0,
			lastCrashTime: this.lastCrashTime || undefined,
			lastSuccessfulRequestTime: this.lastSuccessfulRequestTime || undefined,
			isRunning: !this.crashed && !this.fallback && !!this.process,
			consecutiveFailures: this.consecutiveFailures,
		}
	}

	// ---- Typed API methods ----

	async outline(filePath: string): Promise<DaemonSymbol[]> {
		const resp = await this.send({ command: "outline", file: filePath })
		return resp.symbols || []
	}

	async outlineWithImports(filePath: string): Promise<DaemonOutlineResult> {
		const resp = await this.send({ command: "outline", file: filePath })
		return { symbols: resp.symbols || [], imports: resp.imports || [] }
	}

	async outlineContent(content: string, language: string): Promise<DaemonSymbol[]> {
		const resp = await this.send({ command: "outline", content, language })
		return resp.symbols || []
	}

	async outlineContentWithImports(content: string, language: string): Promise<DaemonOutlineResult> {
		const resp = await this.send({ command: "outline", content, language })
		return { symbols: resp.symbols || [], imports: resp.imports || [] }
	}

	async skeleton(filePath: string): Promise<string> {
		const resp = await this.send({ command: "skeleton", file: filePath })
		return resp.skeleton || ""
	}

	async skeletonContent(content: string, language: string): Promise<string> {
		const resp = await this.send({ command: "skeleton", content, language })
		return resp.skeleton || ""
	}

	async expandSymbol(filePath: string, handle: string): Promise<DaemonExpandResult> {
		const resp = await this.send({ command: "expand-symbol", file: filePath, handle })
		return { body: resp.body, start_line: resp.start_line, end_line: resp.end_line }
	}

	async searchSymbols(query: string, kind?: string, maxResults?: number): Promise<DaemonSearchResult[]> {
		const resp = await this.send({
			command: "search-symbols",
			query,
			...(kind ? { kind } : {}),
			...(maxResults ? { max_results: maxResults } : {}),
		})
		return resp.results || []
	}

	async repoMap(root: string): Promise<any> {
		const resp = await this.send({ command: "repo-map", root })
		return resp.files || []
	}

	async fileChanged(filePath: string): Promise<void> {
		try {
			await this.send({ command: "file-changed", file: filePath })
		} catch {
			// Non-critical — daemon will re-parse on next request anyway
		}
	}

	async batchOutline(files: string[]): Promise<Map<string, DaemonSymbol[]>> {
		const resp = await this.send({ command: "batch", files, subcommand: "outline" })
		const results = new Map<string, DaemonSymbol[]>()
		if (resp.results && Array.isArray(resp.results)) {
			for (const entry of resp.results) {
				if (entry.file && entry.ok) {
					results.set(entry.file, entry.symbols || [])
				}
			}
		}
		return results
	}

	async status(): Promise<any> {
		return this.send({ command: "status" })
	}

	async checkSyntax(filePath: string): Promise<CheckSyntaxResult> {
		const resp = await this.send({ command: "check-syntax", file: filePath })
		return resp.data || { has_errors: false, errors: [] }
	}

	async checkSyntaxContent(content: string, language: string): Promise<CheckSyntaxResult> {
		const resp = await this.send({ command: "check-syntax", content, language })
		return resp.data || { has_errors: false, errors: [] }
	}

	async symbolRange(filePath: string, handle: string): Promise<{
		start_byte: number
		end_byte: number
		start_line: number
		end_line: number
		name_text: string
		handle: string
	} | null> {
		try {
			const resp = await this.send({ command: "symbol-range", file: filePath, handle })
			return resp.data || null
		} catch {
			return null
		}
	}

	async symbolContext(filePath: string, handle: string): Promise<{
		imports: string[]
		class_head: string | null
		properties: string[]
	} | null> {
		try {
			const resp = await this.send({ command: "symbol-context", file: filePath, handle })
			return resp.data || null
		} catch {
			return null
		}
	}

	async indexFile(filePath: string): Promise<{
		symbols: Array<{
			n: string
			t: string
			k?: string
			start_line: number
			start_col: number
			end_line: number
			end_col: number
		}>
		imports: Array<{ module: string; names: string[]; line: number }>
	} | null> {
		try {
			const resp = await this.send({ command: "index-file", file: filePath })
			return resp.data || null
		} catch {
			return null
		}
	}

	async searchIndex(query: string, kind?: string, maxResults?: number): Promise<DaemonSearchResult[]> {
		const resp = await this.send({
			command: "search-index",
			query,
			...(kind ? { kind } : {}),
			...(maxResults ? { max_results: maxResults } : {}),
		})
		return resp.results || []
	}

	async invalidateFileIndex(filePath: string): Promise<void> {
		try {
			await this.send({ command: "invalidate-file", file: filePath })
		} catch {
			// Non-critical
		}
	}

	async indexStatus(): Promise<{ file_count: number; symbol_count: number; import_count: number } | null> {
		try {
			const resp = await this.send({ command: "index-status" })
			return resp.status || null
		} catch {
			return null
		}
	}

	async clearIndex(): Promise<void> {
		try {
			await this.send({ command: "clear-index" })
		} catch {
			// Non-critical
		}
	}

	async indexObservation(type: string, content: string, timestamp: number, tokens: number): Promise<void> {
		try {
			await this.send({
				command: "index-observation",
				type,
				content,
				timestamp,
				tokens,
			})
		} catch (e) {
			Logger.warn("AnalyzerClient", `Failed to index observation: ${e}`)
		}
	}

	async searchObservations(
		query: string,
		limit = 10,
	): Promise<
		Array<{
			type: string
			content: string
			timestamp: number
			tokens: number
		}>
	> {
		try {
			const resp = await this.send({
				command: "search-observations",
				query,
				limit,
			})
			return resp.results || []
		} catch (e) {
			Logger.warn("AnalyzerClient", `Failed to search observations: ${e}`)
			return []
		}
	}

	async indexCriticDecision(text: string, turn: number, confidence: number): Promise<void> {
		try {
			await this.send({
				command: "index-critic-decision",
				content: text,
				turn,
				confidence,
			})
		} catch (e) {
			Logger.warn("AnalyzerClient", `Failed to index critic decision: ${e}`)
		}
	}

	async searchCriticDecisions(query: string, limit = 5): Promise<any[]> {
		try {
			const resp = await this.send({
				command: "search-critic-decisions",
				query,
				limit,
			})
			return resp.results || []
		} catch (e) {
			Logger.warn("AnalyzerClient", `Failed to search critic decisions: ${e}`)
			return []
		}
	}

	async indexWatcherPattern(text: string, fileHash: string, turn: number): Promise<void> {
		try {
			await this.send({
				command: "index-watcher-pattern",
				content: text,
				file_hash: fileHash,
				turn,
			})
		} catch (e) {
			Logger.warn("AnalyzerClient", `Failed to index watcher pattern: ${e}`)
		}
	}

	async searchWatcherPatterns(query: string, limit = 5): Promise<any[]> {
		try {
			const resp = await this.send({
				command: "search-watcher-patterns",
				query,
				limit,
			})
			return resp.results || []
		} catch (e) {
			Logger.warn("AnalyzerClient", `Failed to search watcher patterns: ${e}`)
			return []
		}
	}

	/** Convert daemon symbols to ParsedDefinition[] for backward compatibility */
	static toParsedDefinitions(symbols: DaemonSymbol[], sourceLines?: string[]): ParsedDefinition[] {
		return symbols.map((s) => ({
			id: s.handle,
			kind: s.kind,
			name: s.name,
			lineIndex: s.start_line - 1, // daemon uses 1-based, ParsedDefinition uses 0-based
			text: sourceLines ? sourceLines[s.start_line - 1] || "" : "",
			indentation: "",
			signature: s.signature,
			fullBodyRange: {
				startLine: s.start_line - 1, // convert to 0-based to match WASM behavior
				endLine: s.end_line - 1,
				startIndex: 0,
				endIndex: 0,
			},
		}))
	}
}
