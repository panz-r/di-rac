import * as net from "node:net"
import { EventEmitter } from "node:events"
import { Logger } from "@/shared/services/Logger"

/**
 * CoordinatorClient - TypeScript client for di-vrr-central-deamon
 * Handles hierarchical locking via Unix Domain Sockets.
 */
export class CoordinatorClient extends EventEmitter {
	private static instance: CoordinatorClient | null = null
	private socket: net.Socket | null = null
	private buffer = ""
	private isConnected = false
	private socketPath = "/tmp/di-vrr-coord.sock"
	private responseQueue: Array<(msg: any) => void> = []
	private waiters = new Map<string, Array<() => void>>()
	private connectionPromise: Promise<void> | null = null

	private constructor() {
		super()
	}

	static getInstance(): CoordinatorClient {
		if (!CoordinatorClient.instance) {
			CoordinatorClient.instance = new CoordinatorClient()
		}
		return CoordinatorClient.instance
	}

	async connect(): Promise<void> {
		if (this.isConnected) return
		if (this.connectionPromise) return this.connectionPromise

		this.connectionPromise = new Promise((resolve, reject) => {
			Logger.info(`[CoordinatorClient] Connecting to ${this.socketPath}...`)
			this.socket = net.createConnection(this.socketPath)

			this.socket.on("connect", () => {
				this.isConnected = true
				Logger.info(`[CoordinatorClient] Connected to ${this.socketPath}`)
				resolve()
			})

			this.socket.on("data", (data) => {
				this.handleData(data.toString())
			})

			this.socket.on("error", (err) => {
				Logger.error(`[CoordinatorClient] Socket error:`, err)
				this.isConnected = false
				this.connectionPromise = null
				// Clear queues on error
				this.cleanupQueues()
				reject(err)
			})

			this.socket.on("close", () => {
				Logger.info(`[CoordinatorClient] Connection closed`)
				this.isConnected = false
				this.connectionPromise = null
				this.cleanupQueues()
			})
		})

		return this.connectionPromise
	}

	private cleanupQueues(): void {
		while (this.responseQueue.length > 0) {
			const resolver = this.responseQueue.shift()
			if (resolver) resolver({ status: "error", message: "connection closed" })
		}
		for (const pathWaiters of this.waiters.values()) {
			while (pathWaiters.length > 0) {
				const resolve = pathWaiters.shift()
				if (resolve) resolve()
			}
		}
		this.waiters.clear()
	}

	private handleData(data: string): void {
		this.buffer += data
		let newlineIdx: number
		while ((newlineIdx = this.buffer.indexOf("\n")) !== -1) {
			const line = this.buffer.slice(0, newlineIdx).trim()
			this.buffer = this.buffer.slice(newlineIdx + 1)
			if (line) {
				try {
					const msg = JSON.parse(line)
					if (msg.status === "granted") {
						const path = msg.path
						// If daemon sent "*", we wake up the first waiter (legacy/cleanup behavior)
						const targetPath = (path === "*" || !this.waiters.has(path)) 
							? this.waiters.keys().next().value 
							: path

						if (targetPath) {
							const pathWaiters = this.waiters.get(targetPath)
							if (pathWaiters?.length) {
								pathWaiters.shift()!()
								if (pathWaiters.length === 0) this.waiters.delete(targetPath)
							}
						}
					} else if (msg.status === "config_update") {
						this.emit("config_update", {
							path: msg.path,
							key: msg.key,
							value: msg.value,
						})
					} else {
						// Regular request response
						const resolver = this.responseQueue.shift()
						if (resolver) resolver(msg)
					}
				} catch (e) {
					Logger.error(`[CoordinatorClient] Failed to parse JSON: ${line}`, e)
				}
			}
		}
	}

	/**
	 * Acquire a lock on the given path.
	 * @param path The resource path to lock.
	 * @param wait Whether to wait if the lock is held.
	 * @param timeoutMs Optional timeout in milliseconds.
	 * @returns true if acquired, false if denied or timeout.
	 */
	async acquire(path: string, wait: boolean, timeoutMs = 30000): Promise<boolean> {
		await this.connect()

		return new Promise((resolve) => {
			const timeout = setTimeout(() => {
				// Remove from waiters if still there
				const pathWaiters = this.waiters.get(path)
				if (pathWaiters) {
					const idx = pathWaiters.indexOf(resolveTrue)
					if (idx !== -1) pathWaiters.splice(idx, 1)
				}
				resolve(false)
			}, timeoutMs)

			const resolveTrue = () => {
				clearTimeout(timeout)
				resolve(true)
			}

			this.responseQueue.push((msg) => {
				if (msg.status === "ok") {
					resolveTrue()
				} else if (msg.status === "waiting") {
					if (!wait) {
						clearTimeout(timeout)
						resolve(false)
						return
					}
					const pathWaiters = this.waiters.get(path) || []
					pathWaiters.push(resolveTrue)
					this.waiters.set(path, pathWaiters)
				} else {
					clearTimeout(timeout)
					resolve(false)
				}
			})
			this.socket?.write(JSON.stringify({ method: "acquire", path, wait }) + "\n")
		})
	}

	/**
	 * Set a configuration value in the daemon.
	 * @param path Associative path (e.g. workspace root)
	 * @param key Key name
	 * @param value Value string (null to delete)
	 * @param transient Whether this is a per-connection override
	 */
	async setConfig(path: string, key: string, value: string | null, transient = false): Promise<boolean> {
		await this.connect()
		return new Promise((resolve) => {
			this.responseQueue.push((msg) => {
				resolve(msg.status === "ok")
			})
			this.socket?.write(JSON.stringify({ method: "set_config", path, key, value, transient }) + "\n")
		})
	}

	/**
	 * Get a merged configuration value from the daemon.
	 * Performs lookup: Transient -> Project -> Global.
	 * @param path Associative path
	 * @param key Key name
	 * @returns The effective value string, or null if not set.
	 */
	async getConfig(path: string, key: string): Promise<string | null> {
		await this.connect()
		return new Promise((resolve) => {
			this.responseQueue.push((msg) => {
				if (msg.status === "ok") {
					resolve(msg.value)
				} else {
					resolve(null)
				}
			})
			this.socket?.write(JSON.stringify({ method: "get_config", path, key }) + "\n")
		})
	}

	/**
	 * Release a lock on the given path.
	 * @param path The resource path to release.
	 */
	async release(path: string): Promise<void> {
		try {
			await this.connect()
		} catch (e) {
			return
		}
		return new Promise((resolve) => {
			this.responseQueue.push(() => resolve())
			this.socket?.write(JSON.stringify({ method: "release", path }) + "\n")
		})
	}

	/**
	 * Disconnect from the daemon.
	 */
	dispose(): void {
		if (this.socket) {
			this.socket.destroy()
			this.socket = null
		}
		this.isConnected = false
		this.connectionPromise = null
		this.cleanupQueues()
	}
}
