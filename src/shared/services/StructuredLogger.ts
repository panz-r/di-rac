/**
 * Task-context-enriched logger wrapper.
 *
 * Thin wrapper around the static Logger that automatically attaches
 * taskId, sessionId, and cwd to every log record. Injected into
 * TaskConfig.services so all tool handlers can produce contextual logs.
 */

import { Logger, LogLevel } from "./Logger"
import type { LogRecord } from "./Logger"

export class StructuredLogger {
	private readonly taskId: string
	private readonly sessionId: string
	private readonly cwd: string

	constructor(taskId: string, sessionId: string, cwd: string) {
		this.taskId = taskId
		this.sessionId = sessionId
		this.cwd = cwd
	}

	/**
	 * Set the persistent context on the global Logger so all records
	 * from this task include the task/session identifiers.
	 */
	bind(): void {
		Logger.setContext({ sessionId: this.sessionId, taskId: this.taskId })
	}

	/**
	 * Clear the task context from the global Logger.
	 */
	unbind(): void {
		Logger.clearContext()
	}

	error(message: string, ...args: unknown[]): void {
		Logger.error(message, ...args)
	}

	warn(message: string, ...args: unknown[]): void {
		Logger.warn(message, ...args)
	}

	info(message: string, ...args: unknown[]): void {
		Logger.info(message, ...args)
	}

	debug(message: string, ...args: unknown[]): void {
		Logger.debug(message, ...args)
	}

	trace(message: string, ...args: unknown[]): void {
		Logger.trace(message, ...args)
	}

	/**
	 * Log a structured recovery event.
	 */
	logRecovery(event: { errorCode: string; tool: string; domain: string; recovered: boolean; tier?: string }): void {
		Logger.warn("recovery_event", event)
	}
}
