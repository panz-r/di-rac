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
				reject(err)
			})

			this.socket.on("close", () => {
				Logger.info(`[CoordinatorClient] Connection closed`)
				this.isConnected = false
				this.connectionPromise = null
			})
		})

		return this.connectionPromise
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
	 * @returns true if acquired, false if denied.
	 */
	async acquire(path: string, wait: boolean): Promise<boolean> {
		await this.connect()
		return new Promise((resolve) => {
			this.responseQueue.push((msg) => {
				if (msg.status === "ok") {
					resolve(true)
				} else if (msg.status === "waiting") {
					if (!wait) {
						resolve(false)
						return
					}
					const pathWaiters = this.waiters.get(path) || []
					pathWaiters.push(() => resolve(true))
					this.waiters.set(path, pathWaiters)
				} else {
					resolve(false)
				}
			})
			this.socket?.write(JSON.stringify({ method: "acquire", path, wait }) + "\n")
		})
	}

	/**
	 * Release a lock on the given path.
	 * @param path The resource path to release.
	 */
	async release(path: string): Promise<void> {
		await this.connect()
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
	}
}
