/**
 * CLI shims that provide equivalents to VSCode extension APIs
 * Used by the CLI to maintain compatibility with the core Dirac code
 */

// Re-export EventEmitter from Node.js
import { EventEmitter } from "events"

// Custom wrapper to provide VSCode-style event API on top of EventEmitter
class VsCodeStyleEvent {
	private emitter: EventEmitter
	constructor(emitter: EventEmitter) {
		this.emitter = emitter
	}
	event(handler: (...args: any[]) => void) {
		const self = this
		this.emitter.on("event", handler)
		return {
			// Return disposable-like object
			dispose() {
				self.emitter.off("event", handler)
			},
		}
	}
	fire(...args: any[]) {
		this.emitter.emit("event", ...args)
	}
}

export { EventEmitter }

/**
 * Event emitter for shutdown signals
 * Used by CLI components to gracefully respond to SIGINT/SIGTERM
 */
// Export shutdownEvent with VSCode-style API
export const shutdownEvent = new VsCodeStyleEvent(new EventEmitter()) as any

/**
 * Log file path for CLI
 */
export const CLI_LOG_FILE = (() => {
	const os = require("os")
	const path = require("path")
	const homeDir = os.homedir()
	return path.join(homeDir, ".dirac", "logs", "dirac.log")
})()

/**
 * Window/OutputChannel interface for CLI logging
 */
export const window = {
	createOutputChannel(name: string) {
		return {
			name,
			appendLine(message: string) {
				// Write to log file
				const fs = require("fs")
				const dir = path.dirname(CLI_LOG_FILE)
				if (!fs.existsSync(dir)) {
					fs.mkdirSync(dir, { recursive: true })
				}
				const timestamp = new Date().toISOString()
				fs.appendFileSync(CLI_LOG_FILE, `[${timestamp}] [${name}] ${message}\n`)
			},
			append(_message: string) {
				// No-op for append (not used)
			},
			show() {
				// No-op for show (not used in CLI)
			},
			dispose() {
				// No-op for dispose
			},
		}
	},
}

// Need path module at module scope
import * as path from "path"