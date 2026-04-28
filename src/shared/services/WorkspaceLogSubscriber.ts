/**
 * Workspace file subscriber for the structured Logger.
 *
 * Writes JSON Lines log files to `<workspaceRoot>/.dirac-logs/<sessionId>.log`.
 * Supports file rotation and graceful fallback on disk-full errors.
 */

import { createWriteStream, mkdirSync, readdirSync, renameSync, statSync, unlinkSync, WriteStream } from "node:fs"
import { join } from "node:path"
import { Logger, LogLevel } from "./Logger"
import type { LogRecord } from "./Logger"

export interface WorkspaceLogOptions {
	minLevel?: LogLevel
	maxFileSize?: number // bytes; default 10 MB
	maxFiles?: number // rotated files to keep; default 5
}

export interface WorkspaceLogHandle {
	/** Unsubscribe from Logger and close the write stream. */
	destroy: () => void
}

const DEFAULT_MAX_FILE_SIZE = 10 * 1024 * 1024 // 10 MB
const DEFAULT_MAX_FILES = 5

/**
 * Create a workspace log subscriber and register it with Logger.
 *
 * @returns A handle with a `destroy()` method for cleanup.
 */
export function createWorkspaceLogSubscriber(
	workspaceRoot: string,
	sessionId: string,
	options?: WorkspaceLogOptions,
): WorkspaceLogHandle {
	const minLevel = options?.minLevel ?? LogLevel.INFO
	const maxFileSize = options?.maxFileSize ?? DEFAULT_MAX_FILE_SIZE
	const maxFiles = options?.maxFiles ?? DEFAULT_MAX_FILES

	const logDir = join(workspaceRoot, ".dirac-logs")

	try {
		mkdirSync(logDir, { recursive: true })
	} catch {
		// Directory may already exist or be created concurrently
	}

	const logPath = join(logDir, `${sessionId}.log`)
	let stream: WriteStream | null = createWriteStream(logPath, { flags: "a", encoding: "utf8" })
	let warnedDiskFull = false

	function handleStreamError(err: NodeJS.ErrnoException): void {
		if (err.code === "ENOSPC" && !warnedDiskFull) {
			warnedDiskFull = true
			process.stderr.write(`[dirac-logs] Disk full, logs will not be written to ${logPath}\n`)
			stream?.destroy()
			stream = null
		}
	}

	stream.on("error", handleStreamError)

	function rotate(): void {
		if (!stream) return
		try {
			const stats = statSync(logPath)
			if (stats.size < maxFileSize) return
		} catch {
			return // File may not exist yet
		}

		stream.end()
		stream = null

		// Rotate existing files: .4.log -> delete, .3.log -> .4.log, etc.
		for (let i = maxFiles - 1; i >= 1; i--) {
			const rotatedPath = join(logDir, `${sessionId}.${i}.log`)
			try {
				if (i === maxFiles - 1) {
					unlinkSync(rotatedPath)
				} else {
					renameSync(rotatedPath, join(logDir, `${sessionId}.${i + 1}.log`))
				}
			} catch {
				// File may not exist
			}
		}

		try {
			renameSync(logPath, join(logDir, `${sessionId}.1.log`))
		} catch {
			// Rotation best-effort
		}

		stream = createWriteStream(logPath, { flags: "a", encoding: "utf8" })
		stream.on("error", handleStreamError)
	}

	const subscriber = (_formatted: string, record?: LogRecord): void => {
		if (!record || !stream) return
		if (record.level < minLevel) return

		rotate()

		try {
			stream.write(JSON.stringify(record) + "\n")
		} catch {
			// Best-effort write
		}
	}

	Logger.subscribe(subscriber)

	return {
		destroy(): void {
			Logger.unsubscribe(subscriber)
			if (stream) {
				stream.end()
				stream = null
			}
		},
	}
}
