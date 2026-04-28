/**
 * Structured Logger with level filtering and JSON Lines output.
 *
 * Maintains backward compatibility with all existing callers -- static methods
 * (error, warn, log, info, debug, trace) keep the same signatures.
 * Subscribers now receive both a formatted string and an optional LogRecord.
 */

/** Numeric log levels for filtering. Higher value = higher severity. */
export enum LogLevel {
	TRACE = 0,
	DEBUG = 10,
	INFO = 20,
	LOG = 20,
	WARN = 30,
	ERROR = 40,
}

/** Structured log record passed to subscribers alongside the formatted string. */
export interface LogRecord {
	ts: string
	level: LogLevel
	levelName: string
	message: string
	args: unknown[]
	error?: { message: string; stack?: string }
	source?: string
	sessionId?: string
	taskId?: string
}

type LogSubscriber = (formatted: string, record?: LogRecord) => void

export class Logger {
	private static minLevel: LogLevel = process.env.IS_DEV === "true" ? LogLevel.DEBUG : LogLevel.INFO

	private static subscribers: Set<LogSubscriber> = new Set()

	/** Context fields merged into every LogRecord. */
	private static context: { sessionId?: string; taskId?: string } = {}

	/** Set the minimum log level. Records below this level are discarded. */
	static setMinLevel(level: LogLevel): void {
		Logger.minLevel = level
	}

	/** Get the current minimum log level. */
	static getMinLevel(): LogLevel {
		return Logger.minLevel
	}

	/** Attach persistent context (sessionId, taskId) to every subsequent log record. */
	static setContext(ctx: Partial<{ sessionId: string; taskId: string }>): void {
		Object.assign(Logger.context, ctx)
	}

	/** Clear persistent context. */
	static clearContext(): void {
		Logger.context = {}
	}

	private static output(formatted: string, record?: LogRecord): void {
		for (const subscriber of Logger.subscribers) {
			try {
				subscriber(formatted, record)
			} catch {
				// ignore errors from subscribers
			}
		}
	}

	/**
	 * Register a callback to receive log output messages.
	 * Accepts both new-style (formatted, record?) and legacy (formatted) callbacks.
	 */
	static subscribe(outputFn: LogSubscriber): void {
		Logger.subscribers.add(outputFn)
	}

	/**
	 * Unregister a previously registered log output callback.
	 */
	static unsubscribe(outputFn: LogSubscriber): void {
		Logger.subscribers.delete(outputFn)
	}

	/**
	 * Clear all log output subscribers and context.
	 */
	static reset(): void {
		Logger.subscribers.clear()
		Logger.context = {}
	}

	static error(message: string, ...args: unknown[]) {
		Logger.#emit(LogLevel.ERROR, "ERROR", message, args)
	}

	static warn(message: string, ...args: unknown[]) {
		Logger.#emit(LogLevel.WARN, "WARN", message, args)
	}

	static log(message: string, ...args: unknown[]) {
		Logger.#emit(LogLevel.LOG, "LOG", message, args)
	}

	static debug(message: string, ...args: unknown[]) {
		Logger.#emit(LogLevel.DEBUG, "DEBUG", message, args)
	}

	static info(message: string, ...args: unknown[]) {
		Logger.#emit(LogLevel.INFO, "INFO", message, args)
	}

	static trace(message: string, ...args: unknown[]) {
		Logger.#emit(LogLevel.TRACE, "TRACE", message, args)
	}

	static #emit(level: LogLevel, levelName: string, message: string, args: unknown[]): void {
		if (level < Logger.minLevel) {
			return
		}

		try {
			// Extract error object from args for structured error field
			let errorInfo: LogRecord["error"]
			const serializedArgs: unknown[] = []

			for (const arg of args) {
				if (arg instanceof Error) {
					errorInfo = { message: arg.message, stack: arg.stack }
					serializedArgs.push(`${arg.message}${arg.stack ? `\n${arg.stack}` : ""}`)
				} else {
					serializedArgs.push(arg)
				}
			}

			// Build formatted string for backward compatibility
			let fullMessage = message
			if (serializedArgs.length > 0) {
				fullMessage += ` ${serializedArgs
					.map((arg) => {
						try {
							return typeof arg === "object" ? JSON.stringify(arg) : String(arg)
						} catch {
							return String(arg)
						}
					})
					.join(" ")}`
			}

			const formatted = `${levelName} ${fullMessage}`.trimEnd()

			// Build structured record
			const record: LogRecord = {
				ts: new Date().toISOString(),
				level,
				levelName,
				message: fullMessage.trimEnd(),
				args,
				...errorInfo && { error: errorInfo },
				...Logger.context,
			}

			Logger.output(formatted, record)
		} catch {}
	}
}
