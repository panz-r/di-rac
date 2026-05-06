import { getSavedDiracMessages, getTaskMetadata, readTaskHistoryFromState, writeTaskHistoryToState } from "@core/storage/disk"
import { HostProvider } from "@hosts/host-provider"
import { DiracMessage } from "@shared/ExtensionMessage"
import { HistoryItem } from "@shared/HistoryItem"
import { ShowMessageType } from "@shared/proto/host/window"
import { fileExistsAtPath } from "@utils/fs"
import * as path from "path"
import { ulid } from "ulid"
import { Logger } from "@/shared/services/Logger"

interface TaskReconstructionResult {
	totalTasks: number
	reconstructedTasks: number
	skippedTasks: number
	errors: string[]
}

/**
 * Reconstructs task history from existing task folders
 * @param showProgress Whether to show user-facing progress notifications
 * @returns Reconstruction result or null if cancelled
 */
export async function reconstructTaskHistory(showProgress = true): Promise<TaskReconstructionResult | null> {
	try {
		// Show confirmation dialog using HostProvider (always shown for user consent)
		const proceed = await HostProvider.window.showMessage({
			type: ShowMessageType.WARNING,
			message:
				"This will rebuild your task history from existing task data. This operation will backup your current task history and attempt to reconstruct it from task folders. Continue?",
			options: {
				items: ["Yes, Reconstruct", "Cancel"],
			},
		})

		if (proceed?.selectedOption !== "Yes, Reconstruct") {
			return null
		}

		if (showProgress) {
			// Show initial progress message
			HostProvider.window.showMessage({
				type: ShowMessageType.INFORMATION,
				message: "Reconstructing task history...",
			})
		}

		const result = await performTaskHistoryReconstruction()

		// Show results
		if (showProgress) {
			if (result.errors.length > 0) {
				const errorMessage = `Reconstruction completed with warnings:\n- Reconstructed: ${result.reconstructedTasks} tasks\n- Skipped: ${result.skippedTasks} tasks\n- Errors: ${result.errors.length}\n\nFirst few errors:\n${result.errors.slice(0, 3).join("\n")}`

				HostProvider.window.showMessage({
					type: ShowMessageType.WARNING,
					message: errorMessage,
				})
			} else {
				HostProvider.window.showMessage({
					type: ShowMessageType.INFORMATION,
					message: `Task history successfully reconstructed! Found and restored ${result.reconstructedTasks} tasks.`,
				})
			}
		}

		return result
	} catch (error) {
		const errorMessage = error instanceof Error ? error.message : String(error)
		if (showProgress) {
			HostProvider.window.showMessage({
				type: ShowMessageType.ERROR,
				message: `Failed to reconstruct task history: ${errorMessage}`,
			})
		}
		return null
	}
}

async function performTaskHistoryReconstruction(): Promise<TaskReconstructionResult> {
	const result: TaskReconstructionResult = {
		totalTasks: 0,
		reconstructedTasks: 0,
		skippedTasks: 0,
		errors: [],
	}

	// Backup existing task history
	await backupExistingTaskHistory()

	// Get tasks directory
	const tasksDir = path.join(HostProvider.get().globalStorageFsPath, "tasks")

	// Check if tasks directory exists
	if (!(await fileExistsAtPath(tasksDir))) {
		throw new Error("No tasks directory found. Nothing to reconstruct.")
	}

	// Scan for task directories
	const taskIds = await scanTaskDirectories(tasksDir)
	result.totalTasks = taskIds.length

	if (taskIds.length === 0) {
		throw new Error("No task directories found. Nothing to reconstruct.")
	}

	// Process each task
	const reconstructedItems: HistoryItem[] = []

	for (const taskId of taskIds) {
		try {
			const historyItem = await reconstructTaskHistoryItem(taskId)
			if (historyItem) {
				reconstructedItems.push(historyItem)
				result.reconstructedTasks++
			} else {
				result.skippedTasks++
			}
		} catch (error) {
			result.skippedTasks++
			const errorMsg = error instanceof Error ? error.message : String(error)
			result.errors.push(`Task ${taskId}: ${errorMsg}`)
		}
	}

	// Sort by timestamp (newest first), with ID as tiebreaker for stability
	reconstructedItems.sort((a, b) => {
		if (b.ts !== a.ts) {
			return b.ts - a.ts
		}
		return b.id.localeCompare(a.id)
	})

	// Write reconstructed history
	await writeTaskHistoryToState(reconstructedItems)

	return result
}

async function backupExistingTaskHistory(): Promise<void> {
	try {
		const existingHistory = await readTaskHistoryFromState()
		if (existingHistory.length > 0) {
			const backupPath = path.join(HostProvider.get().globalStorageFsPath, "state", `taskHistory.backup.${Date.now()}.json`)

			// Ensure state directory exists
			const fs = await import("fs/promises")
			await fs.mkdir(path.dirname(backupPath), { recursive: true })
			await fs.writeFile(backupPath, JSON.stringify(existingHistory, null, 2))
		}
	} catch (error) {
		// Non-fatal error, just log it
		Logger.warn("Failed to backup existing task history:", error)
	}
}

async function scanTaskDirectories(tasksDir: string): Promise<string[]> {
	const fs = await import("fs/promises")

	try {
		const entries = await fs.readdir(tasksDir, { withFileTypes: true })
		return entries
			.filter((entry) => entry.isDirectory())
			.map((entry) => entry.name)
			.filter((name) => /^[^.]+$/.test(name)) // Accept any non-dot directory
	} catch (error) {
		throw new Error(`Failed to scan tasks directory: ${error}`)
	}
}

async function reconstructTaskHistoryItem(taskId: string): Promise<HistoryItem | null> {
	try {
		// Load UI messages to extract task info
		const diracMessages = await getSavedDiracMessages(taskId)
		if (diracMessages.length === 0) {
			return null // Skip empty tasks
		}

		// Load task metadata for token usage
		const metadata = await getTaskMetadata(taskId)

		// Extract task information
		const taskInfo = extractTaskInformation(diracMessages, metadata)

		// Create HistoryItem
		const historyItem: HistoryItem = {
			id: taskId,
			ulid: taskInfo.ulid || ulid(), // Generate new ULID if missing
			ts: taskInfo.timestamp,
			task: taskInfo.taskDescription,
			tokensIn: taskInfo.tokensIn,
			tokensOut: taskInfo.tokensOut,
			cacheWrites: taskInfo.cacheWrites,
			cacheReads: taskInfo.cacheReads,
			size: taskInfo.size,
			isFavorited: taskInfo.isFavorited,
			conversationHistoryDeletedRange: taskInfo.conversationHistoryDeletedRange,
		}

		return historyItem
	} catch (error) {
		throw new Error(`Failed to reconstruct task ${taskId}: ${error}`)
	}
}

interface TaskInfo {
	ulid?: string
	timestamp: number
	taskDescription: string
	tokensIn: number
	tokensOut: number
	cacheWrites?: number
	cacheReads?: number
	size?: number
	isFavorited?: boolean
	conversationHistoryDeletedRange?: [number, number]
}

function extractTaskInformation(diracMessages: DiracMessage[], metadata: any): TaskInfo {
	// Find the first user message (task description)
	const firstUserMessage = diracMessages.find(
		(msg) => msg.type === "say" && (msg.say === "task" || msg.say === "text") && msg.text,
	)

	// Extract timestamp from first task or user message or use first message in array
	const timestamp = firstUserMessage?.ts ?? (diracMessages.length > 0 ? diracMessages[0].ts : Date.now())

	// Extract task description
	let taskDescription = "Untitled Task"
	if (firstUserMessage?.text) {
		// Clean up the task description
		const cleanText = firstUserMessage.text
			.replace(/<task>\s*/g, "")
			.replace(/\s*<\/task>/g, "")
			.trim()

		const firstLine = cleanText.split("\n")[0]
		if (firstLine) {
			taskDescription = firstLine.substring(0, 100) // Limit length
		}
	}

	// Calculate token usage from API request messages
	let tokensIn = 0
	let tokensOut = 0
	let cacheWrites = 0
	let cacheReads = 0
	let parseFailures = 0

	// Look for usage-carrying messages with token info
	const apiReqMessages = diracMessages.filter(
		(msg) => msg.type === "say" && (msg.say === "api_req_started" || msg.say === "subagent_usage") && msg.text,
	)

	for (const msg of apiReqMessages) {
		try {
			if (msg.text) {
				const apiInfo = JSON.parse(msg.text) as unknown
				if (!apiInfo || typeof apiInfo !== "object") {
					continue
				}

				const usage = apiInfo as Record<string, unknown>
				const getVal = (keys: string[]) => {
					for (const key of keys) {
						if (typeof usage[key] === "number" && Number.isFinite(usage[key])) {
							return usage[key] as number
						}
					}
					return 0
				}

				tokensIn += getVal(["tokensIn", "inputTokens", "input_tokens"])
				tokensOut += getVal(["tokensOut", "outputTokens", "output_tokens"])
				cacheWrites += getVal(["cacheWrites", "cacheCreationInputTokens", "cache_creation_input_tokens"])
				cacheReads += getVal(["cacheReads", "cacheReadInputTokens", "cache_read_input_tokens"])
			}
		} catch {
			parseFailures++
		}
	}

	if (parseFailures > 0) {
		Logger.debug(`Skipped ${parseFailures} unparseable API messages for task history reconstruction`)
	}

	// Use metadata if available and no tokens found in messages
	if (tokensIn === 0 && tokensOut === 0 && metadata.model_usage && Array.isArray(metadata.model_usage)) {
		for (const usage of metadata.model_usage) {
			tokensIn += usage.tokensIn || 0
			tokensOut += usage.tokensOut || 0
			cacheWrites += usage.cacheWrites || 0
			cacheReads += usage.cacheReads || 0
		}
	}

	// Extract ULID if available
	const ulid = metadata?.ulid || undefined

	// Calculate approximate size (rough estimate)
	const messageSize = JSON.stringify(diracMessages).length
	const size = Math.floor(messageSize / 1024) // KB

	return {
		ulid,
		timestamp,
		taskDescription,
		tokensIn,
		tokensOut,
		cacheWrites: cacheWrites > 0 ? cacheWrites : undefined,
		cacheReads: cacheReads > 0 ? cacheReads : undefined,
		size,
	}
}
