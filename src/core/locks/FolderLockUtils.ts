import { Logger } from "@/shared/services/Logger"
import { CoordinatorClient } from "./CoordinatorClient"
import type { FolderLockOptions, FolderLockResult, FolderLockWithRetryResult } from "./types"

/**
 * Retry configuration for folder lock acquisition
 */
export interface FolderLockRetryConfig {
	initialDelayMs: number
	incrementPerAttemptMs: number
	maxTotalTimeoutMs: number
}

/**
 * Default retry configuration for folder locks:
 * - 500ms initial wait - this is typically enough for most cases
 * - +1s backoff per attempt
 * - 30s max total timeout
 */
export const DEFAULT_RETRY_CONFIG: FolderLockRetryConfig = {
	initialDelayMs: 500,
	incrementPerAttemptMs: 1000,
	maxTotalTimeoutMs: 30000,
}

/**
 * Get the coordinator client instance.
 */
export function getCoordinatorClient(): CoordinatorClient {
	return CoordinatorClient.getInstance()
}

/**
 * Attempt to acquire a folder lock with retry logic.
 * With the C coordination daemon, 'wait' is handled natively by the server.
 *
 * @param lockTarget - The folder path to lock
 * @param config - Optional retry configuration (mostly ignored now as daemon handles waiting)
 * @returns Promise<boolean> true if lock acquired, false if timeout/denied
 */
export async function tryAcquireFolderLockWithRetry(
	options: FolderLockOptions,
	_config?: FolderLockRetryConfig,
): Promise<FolderLockWithRetryResult> {
	try {
		const client = getCoordinatorClient()
		const acquired = await client.acquire(options.lockTarget, true)
		return { acquired, skipped: false }
	} catch (error) {
		// Daemon not running — skip locking (single-instance mode)
		Logger.log("Coordinator daemon unavailable, skipping lock")
		return { acquired: false, skipped: true }
	}
}

/**
 * Release a folder lock safely with error handling.
 *
 * @param taskId - Task ID (ignored by coordinator daemon which uses FD tracking)
 * @param lockTarget - The folder path to release
 */
export async function releaseFolderLock(_taskId: string, lockTarget: string): Promise<void> {
	try {
		const client = getCoordinatorClient()
		await client.release(lockTarget)
		Logger.log(`Released folder lock for: ${lockTarget}`)
	} catch (error) {
		Logger.error("Error releasing folder lock:", error)
	}
}

/**
 * Acquire a folder lock with no retry
 * @param options - Folder lock options including heldBy
 * @returns Result indicating if lock was acquired
 */
export async function acquireFolderLock(options: FolderLockOptions): Promise<FolderLockResult> {
	try {
		const client = getCoordinatorClient()
		const acquired = await client.acquire(options.lockTarget, false)
		return { acquired }
	} catch (error) {
		Logger.error("Failed to acquire folder lock:", error)
		return { acquired: false }
	}
}
