import { ChildProcess, spawn } from "child_process"
import * as path from "path"
import * as fs from "fs"
import { Logger } from "@/shared/services/Logger"

export interface CommandResult {
	type: "result"
	id: string
	stdout: string
	stderr: string
	exit_code: number
	meta: {
		mode_used: string
		cwd: string
		truncated: boolean
		truncation_offset: number | null
		hint: string | null
		blocked: string | null
		timed_out: boolean
		detected_patterns: string[]
	}
}

export interface SessionInfoResult {
	type: "session_info_result"
	id: string
	session_id: string
	cwd: string
	env: Record<string, string>
}

export interface WalkResult {
	type: "walk_result"
	id: string
	files: Array<{
		path: string
		mtime: number
		size: number
	}>
}

interface PendingRequest {
	resolve: (value: any) => void
	reject: (reason: any) => void
	timer: ReturnType<typeof setTimeout>
}

export class CommandClient {
	private process: ChildProcess | null = null
	private requestId = 0
	private pending = new Map<number, PendingRequest>()
	private buffer = ""
	private binaryPath: string
	private workspaceRoot: string
	private crashed = false
	private shuttingDown = false

	/** Set to true if the daemon cannot be started */
	fallback = true

	private readyResolvers: Array<() => void> = []
	private restartCount = 0
	private restartTimer: ReturnType<typeof setTimeout> | null = null

	constructor(binaryPath: string, workspaceRoot: string) {
		this.binaryPath = binaryPath
		this.workspaceRoot = workspaceRoot
	}

	async start(): Promise<void> {
		if (this.shuttingDown) return
		if (this.process && !this.process.killed && !this.crashed) return
		if (!fs.existsSync(this.binaryPath)) {
			throw new Error(`Command daemon binary not found: ${this.binaryPath}. Cannot start without it.`)
		}
		try {
			await this.spawnDaemon()
			this.fallback = false
			Logger.info("CommandClient", "Daemon started")
		} catch (e) {
			throw new Error(`Command daemon failed to start: ${e}`)
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
			})

			proc.stderr!.on("data", (chunk: Buffer) => {
				const msg = chunk.toString().trim()
				if (msg.includes("ready") && !startupResolved) {
					startupResolved = true
					resolve()
				}
			})

			proc.on("error", (err) => {
				Logger.warn("CommandClient", `Daemon error: ${err.message}`)
				if (!startupResolved) {
					startupResolved = true
					reject(err)
				}
				this.handleCrash()
			})

			proc.on("exit", (code) => {
				Logger.info("CommandClient", `Daemon exited with code ${code}`)
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
		this.buffer = lines.pop() || ""

		for (const line of lines) {
			if (!line.trim()) continue
			try {
				const msg = JSON.parse(line)
				if (msg.id !== undefined) {
					const pending = this.pending.get(msg.id)
					if (pending) {
						if (msg.type === "ack") {
							// Ack received \u2014 set post-ack timer based on daemon timeout_ms
							clearTimeout(pending.timer)
							const daemonTimeout = typeof msg.timeout_ms === "number" ? msg.timeout_ms : 30000
							const postAckMs = daemonTimeout + 15000 // buffer for process cleanup
							pending.timer = setTimeout(() => {
								this.pending.delete(msg.id)
								pending.reject(new Error(`Request ${msg.id} timed out after no response following ack`))
							}, postAckMs)
						} else if (msg.type === "error") {
							clearTimeout(pending.timer)
							this.pending.delete(msg.id)
							pending.reject(new Error(msg.message || "Daemon error"))
						} else {
							clearTimeout(pending.timer)
							this.pending.delete(msg.id)
							pending.resolve(msg)
						}
					}
				}
			} catch {
				Logger.warn("CommandClient", `Failed to parse daemon response: ${line.slice(0, 200)}`)
			}
		}
	}

	private handleCrash(): void {
		// Guard against double-fire from both 'error' and 'exit' events
		if (this.crashed && !this.process) return
		this.crashed = true
		// Reject all pending requests
		for (const [id, pending] of this.pending) {
			clearTimeout(pending.timer)
			pending.reject(new Error("Daemon crashed"))
		}
		this.pending.clear()
		this.process = null

		if (this.shuttingDown) return

		// Clear any existing restart timer before scheduling a new one
		if (this.restartTimer) {
			clearTimeout(this.restartTimer)
			this.restartTimer = null
		}

		// Schedule restart with exponential backoff
		this.restartCount++
		const delay = Math.min(2000 * Math.pow(2, Math.min(this.restartCount, 5)), 60000)
		Logger.info("CommandClient", `Scheduling daemon restart in ${delay}ms (attempt ${this.restartCount})`)

		this.restartTimer = setTimeout(() => {
			this.start().then(() => {
				Logger.info("CommandClient", "Daemon restarted")
				this.crashed = false
				const resolvers = this.readyResolvers
				this.readyResolvers = []
				resolvers.forEach((r) => r())
			}).catch((e) => {
				Logger.warn("CommandClient", `Restart failed: ${e}`)
			})
		}, delay)
	}

	async shutdown(): Promise<void> {
		this.shuttingDown = true
		if (this.restartTimer) {
			clearTimeout(this.restartTimer)
			this.restartTimer = null
		}
		if (this.process && !this.process.killed) {
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
				this.readyResolvers = this.readyResolvers.filter((r) => r !== onReady)
				reject(new Error("Timed out waiting for daemon to become ready"))
			}, timeoutMs)
			const onReady = () => {
				clearTimeout(timer)
				resolve()
			}
			this.readyResolvers.push(onReady)
		})
	}

	private send(payload: Record<string, unknown>, timeoutMs = 30000): Promise<any> {
		return new Promise((resolve, reject) => {
			if (this.shuttingDown) {
				reject(new Error("Daemon shut down"))
				return
			}
			if (!this.process || this.process.killed || this.crashed) {
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
			const message = JSON.stringify({ id: String(id), ...payload }) + "\n"
			const timer = setTimeout(() => {
				this.pending.delete(id)
				reject(new Error(`Request ${id} timed out after ${timeoutMs}ms`))
			}, timeoutMs)
			this.pending.set(id, { resolve, reject, timer })
			this.process.stdin!.write(message)
		})
	}

	async execute(command: string, sessionId?: string, timeoutSeconds?: number): Promise<CommandResult> {
		return this.send({
			type: "execute",
			command,
			session_id: sessionId || undefined,
			timeout: timeoutSeconds || undefined,
		})
	}

	async sessionInfo(sessionId: string): Promise<SessionInfoResult> {
		return this.send({
			type: "session_info",
			session_id: sessionId,
		})
	}

	async walk(dir?: string): Promise<WalkResult> {
		return this.send(
			{
				type: "walk",
				dir: dir || undefined,
			},
			30000,
		)
	}
}
