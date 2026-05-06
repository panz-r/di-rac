import { Anthropic } from "@anthropic-ai/sdk"
import { TaskMetadata } from "@core/context/context-tracking/ContextTrackerTypes"
import { execa } from "@packages/execa"
import { DiracMessage } from "@shared/ExtensionMessage"
import { HistoryItem } from "@shared/HistoryItem"
import { RemoteConfig } from "@shared/remote-config/schema"
import { GlobalState, Settings } from "@shared/storage/state-keys"
import { fileExistsAtPath, isDirectory } from "@utils/fs"
import fs from "fs/promises"
import os from "os"
import * as path from "path"
import { HostProvider } from "@/hosts/host-provider"
import { ExtensionRegistryInfo } from "@/registry"
import { Logger } from "@/shared/services/Logger"
import { syncWorker } from "@/shared/services/worker/sync"
import { reconstructTaskHistory } from "../commands/reconstructTaskHistory"
import { StateManager } from "./StateManager"

/**
 * Atomically write data to a file using temp file + rename pattern.
 * This prevents readers from seeing partial/incomplete data by writing to a temporary
 * file first, then renaming it to the target location. The rename operation is atomic
 * in most cases on modern systems, though behavior may vary across platforms and filesystems.
 *
 * @param filePath - The target file path
 * @param data - The data to write
 */
async function atomicWriteFile(filePath: string, data: string): Promise<void> {
	const tmpPath = `${filePath}.tmp.${Date.now()}.${Math.random().toString(36).substring(7)}.json`
	try {
		// Write to temporary file first
		await fs.writeFile(tmpPath, data, "utf8")
		// Rename temp file to target (atomic in most cases)
		await fs.rename(tmpPath, filePath)
	} catch (error) {
		// Clean up temp file if it exists
		fs.unlink(tmpPath).catch(() => {})
		throw error
	}
}

export const GlobalFileNames = {
	apiConversationHistory: "api_conversation_history.json",
	contextHistory: "context_history.json",
	uiMessages: "ui_messages.json",
	diracRules: ".diracrules",
	workflows: ".diracrules/workflows",
	hooksDir: ".diracrules/hooks",
	diracruleSkillsDir: ".diracrules/skills",
	diracSkillsDir: ".dirac/skills",
	claudeSkillsDir: ".claude/skills",
	agentsSkillsDir: ".agents/skills",
	aiSkillsDir: ".ai/skills",
	cursorRulesDir: ".cursor/rules",
	cursorRulesFile: ".cursorrules",
	windsurfRules: ".windsurfrules",
	agentsRulesFile: "AGENTS.md",
	taskMetadata: "task_metadata.json",
	sessionMeta: "session_meta.json",
	fileManifest: "file_manifest.json",
	remoteConfig: (orgId: string) => `remote_config_${orgId}.json`,
}

let _cachedDocumentsPath: string | undefined

export async function getDocumentsPath(): Promise<string> {
	if (_cachedDocumentsPath) {
		return _cachedDocumentsPath
	}

	let result: string
	if (process.platform === "win32") {
		try {
			const { stdout: docsPath } = await execa("powershell", [
				"-NoProfile", // Ignore user's PowerShell profile(s)
				"-Command",
				"[System.Environment]::GetFolderPath([System.Environment+SpecialFolder]::MyDocuments)",
			])
			const trimmedPath = docsPath.trim()
			if (trimmedPath) {
				result = trimmedPath
			} else {
				result = path.join(os.homedir(), "Documents")
			}
		} catch (_err) {
			result = path.join(os.homedir(), "Documents")
		}
	} else if (process.platform === "linux") {
		try {
			await execa("which", ["xdg-user-dir"])
			const { stdout } = await execa("xdg-user-dir", ["DOCUMENTS"])
			const trimmedPath = stdout.trim()
			if (trimmedPath) {
				result = trimmedPath
			} else {
				result = path.join(os.homedir(), "Documents")
			}
		} catch {
			result = path.join(os.homedir(), "Documents")
		}
	} else {
		result = path.join(os.homedir(), "Documents")
	}

	_cachedDocumentsPath = result
	return result
}

/**
 * Returns the cross-platform path to the Dirac home directory (~/.dirac).
 * This works on macOS, Linux, and Windows:
 * - macOS: /Users/username/.dirac
 * - Linux: /home/username/.dirac
 * - Windows: C:\Users\username\.dirac
 *
 * This is intended to eventually replace ~/Documents/Dirac as the global config location.
 */
export function getDiracHomePath(): string {
	return path.join(os.homedir(), ".dirac")
}

export async function ensureTaskDirectoryExists(taskId: string): Promise<string> {
	return getGlobalStorageDir("tasks", taskId)
}

export async function ensureRulesDirectoryExists(): Promise<string> {
	const userDocumentsPath = await getDocumentsPath()
	const diracRulesDir = path.join(userDocumentsPath, "Dirac", "Rules")
	try {
		await fs.mkdir(diracRulesDir, { recursive: true })
	} catch (_error) {
		return path.join(os.homedir(), "Documents", "Dirac", "Rules") // in case creating a directory in documents fails for whatever reason (e.g. permissions) - this is fine because we will fail gracefully with a path that does not exist
	}
	return diracRulesDir
}

export async function ensureWorkflowsDirectoryExists(): Promise<string> {
	const userDocumentsPath = await getDocumentsPath()
	const diracWorkflowsDir = path.join(userDocumentsPath, "Dirac", "Workflows")
	try {
		await fs.mkdir(diracWorkflowsDir, { recursive: true })
	} catch (_error) {
		return path.join(os.homedir(), "Documents", "Dirac", "Workflows") // in case creating a directory in documents fails for whatever reason (e.g. permissions) - this is fine because we will fail gracefully with a path that does not exist
	}
	return diracWorkflowsDir
}

export async function ensureHooksDirectoryExists(): Promise<string> {
	const userDocumentsPath = await getDocumentsPath()
	const diracHooksDir = path.join(userDocumentsPath, "Dirac", "Hooks")
	try {
		await fs.mkdir(diracHooksDir, { recursive: true })
	} catch (_error) {
		return path.join(os.homedir(), "Documents", "Dirac", "Hooks") // in case creating a directory in documents fails for whatever reason (e.g. permissions) - this is fine because we will fail gracefully with a path that does not exist
	}
	return diracHooksDir
}

/**
 * Returns the global skills directory path (~/.dirac/skills) without creating it.
 */
function getDiracSkillsDirectoryPath(): string {
	return path.join(getDiracHomePath(), "skills")
}

function getAgentSkillsDirectoryPath(): string {
	return path.join(os.homedir(), ".agents", "skills")
}

/**
 * Returns the global Claude skills directory path (~/.claude/skills).
 */
function getClaudeSkillsDirectoryPath(): string {
	return path.join(os.homedir(), ".claude", "skills")
}

/**
 * Returns the global AI skills directory path (~/.ai/skills).
 */
function getAiSkillsDirectoryPath(): string {
	return path.join(os.homedir(), ".ai", "skills")
}


/**
 * Returns the global agent skills directory path (~/.agents/skills).
 * Creates the directory if it doesn't exist.
 * This is the opinionated location for new global skills.
 */
export async function ensureAgentSkillsDirectoryExists(options: { isGlobal: boolean; workspacePath?: string }): Promise<string> {
	const agentSkillsDir = options.isGlobal
		? getAgentSkillsDirectoryPath()
		: path.join(options.workspacePath ?? "", GlobalFileNames.agentsSkillsDir)
	try {
		await fs.mkdir(agentSkillsDir, { recursive: true })
	} catch (_error) {
		// Fallback - return the path even if mkdir fails, we'll fail gracefully later
		return agentSkillsDir
	}
	return agentSkillsDir
}

export type SkillsScanDirectory = {
	path: string
	source: "project" | "global"
}

/**
 * Returns the list of skills directories to scan without creating them.
 * Order is project directories first, then global directories.
 */
export function getSkillsDirectoriesForScan(cwd: string): SkillsScanDirectory[] {
	return [
		{ path: path.join(cwd, GlobalFileNames.diracruleSkillsDir), source: "project" },
		{ path: path.join(cwd, GlobalFileNames.diracSkillsDir), source: "project" },
		{ path: path.join(cwd, GlobalFileNames.claudeSkillsDir), source: "project" },
		{ path: path.join(cwd, GlobalFileNames.aiSkillsDir), source: "project" },
		{ path: path.join(cwd, GlobalFileNames.agentsSkillsDir), source: "project" },
		{ path: getDiracSkillsDirectoryPath(), source: "global" },
		{ path: getAgentSkillsDirectoryPath(), source: "global" },
		{ path: getClaudeSkillsDirectoryPath(), source: "global" },
		{ path: getAiSkillsDirectoryPath(), source: "global" },
	]
}

export async function ensureSettingsDirectoryExists(): Promise<string> {
	return getGlobalStorageDir("settings")
}

export async function getSavedApiConversationHistory(taskId: string): Promise<Anthropic.MessageParam[]> {
	const filePath = path.join(await ensureTaskDirectoryExists(taskId), GlobalFileNames.apiConversationHistory)
	const fileExists = await fileExistsAtPath(filePath)
	if (fileExists) {
		return JSON.parse(await fs.readFile(filePath, "utf8"))
	}
	return []
}

export async function saveApiConversationHistory(taskId: string, apiConversationHistory: Anthropic.MessageParam[]) {
	try {
		if (apiConversationHistory.length > 0) {
			const fileName = GlobalFileNames.apiConversationHistory
			const data = JSON.stringify(apiConversationHistory)
			// Queue for remote sync without blocking
			syncWorker().enqueue(taskId, fileName, data)
			// Store locally
			const filePath = path.join(await ensureTaskDirectoryExists(taskId), fileName)
			await atomicWriteFile(filePath, data)
		}
	} catch (error) {
		// in the off chance this fails, we don't want to stop the task
		Logger.error("Failed to save API conversation history:", error)
	}
}

export async function getSavedDiracMessages(taskId: string): Promise<DiracMessage[]> {
	const filePath = path.join(await ensureTaskDirectoryExists(taskId), GlobalFileNames.uiMessages)
	if (await fileExistsAtPath(filePath)) {
		return JSON.parse(await fs.readFile(filePath, "utf8"))
	}
	// check old location
	const oldPath = path.join(await ensureTaskDirectoryExists(taskId), "claude_messages.json")
	if (await fileExistsAtPath(oldPath)) {
		const data = JSON.parse(await fs.readFile(oldPath, "utf8"))
		await fs.unlink(oldPath) // remove old file
		return data
	}
	return []
}

export async function saveDiracMessages(taskId: string, uiMessages: DiracMessage[]) {
	try {
		const taskDir = await ensureTaskDirectoryExists(taskId)
		const filePath = path.join(taskDir, GlobalFileNames.uiMessages)
		await atomicWriteFile(filePath, JSON.stringify(uiMessages))
	} catch (error) {
		Logger.error("Failed to save ui messages:", error)
	}
}

export async function getTaskMetadata(taskId: string): Promise<TaskMetadata> {
	const filePath = path.join(await ensureTaskDirectoryExists(taskId), GlobalFileNames.taskMetadata)
	try {
		if (await fileExistsAtPath(filePath)) {
			return JSON.parse(await fs.readFile(filePath, "utf8"))
		}
	} catch (error) {
		Logger.error("Failed to read task metadata:", error)
	}
	return { files_in_context: [], model_usage: [], environment_history: [] }
}

export async function saveTaskMetadata(taskId: string, metadata: TaskMetadata) {
	try {
		const taskDir = await ensureTaskDirectoryExists(taskId)
		const filePath = path.join(taskDir, GlobalFileNames.taskMetadata)
		await fs.writeFile(filePath, JSON.stringify(metadata, null, 2))
	} catch (error) {
		Logger.error("Failed to save task metadata:", error)
	}
}

export interface SessionMeta {
	options: Record<string, unknown>
	cmdline: string
	savedAt: string
}

export async function writeSessionMeta(taskId: string, meta: SessionMeta): Promise<void> {
	try {
		const taskDir = await ensureTaskDirectoryExists(taskId)
		const filePath = path.join(taskDir, GlobalFileNames.sessionMeta)
		await atomicWriteFile(filePath, JSON.stringify(meta, null, 2))
	} catch (error) {
		Logger.error("Failed to write session meta:", error)
	}
}

export async function readSessionMeta(taskId: string): Promise<SessionMeta | null> {
	try {
		const filePath = path.join(await ensureTaskDirectoryExists(taskId), GlobalFileNames.sessionMeta)
		if (await fileExistsAtPath(filePath)) {
			return JSON.parse(await fs.readFile(filePath, "utf8"))
		}
	} catch (error) {
		Logger.error("Failed to read session meta:", error)
	}
	return null
}

export interface FileManifestEntry {
	hash: string
	mtimeMs: number
}

export interface FileManifest {
	savedAt: string
	files: Record<string, FileManifestEntry>
}

export async function writeFileManifest(taskId: string, manifest: FileManifest): Promise<void> {
	try {
		const taskDir = await ensureTaskDirectoryExists(taskId)
		const filePath = path.join(taskDir, GlobalFileNames.fileManifest)
		await atomicWriteFile(filePath, JSON.stringify(manifest, null, 2))
	} catch (error) {
		Logger.error("Failed to write file manifest:", error)
	}
}

export async function readFileManifest(taskId: string): Promise<FileManifest | null> {
	try {
		const filePath = path.join(await ensureTaskDirectoryExists(taskId), GlobalFileNames.fileManifest)
		if (await fileExistsAtPath(filePath)) {
			return JSON.parse(await fs.readFile(filePath, "utf8"))
		}
	} catch (error) {
		Logger.error("Failed to read file manifest:", error)
	}
	return null
}

export interface FileChangeResult {
	changed: string[]
	deleted: string[]
	unchanged: number
}

export async function detectFileChanges(taskId: string, cwd: string): Promise<FileChangeResult> {
	const manifest = await readFileManifest(taskId)
	if (!manifest || Object.keys(manifest.files).length === 0) {
		return { changed: [], deleted: [], unchanged: 0 }
	}

	const changed: string[] = []
	const deleted: string[] = []
	let unchanged = 0

	for (const [relPath, entry] of Object.entries(manifest.files)) {
		const absPath = path.resolve(cwd, relPath)
		try {
			const stats = await fs.stat(absPath)
			if (stats.mtimeMs === entry.mtimeMs) {
				unchanged++
				continue
			}
			const content = await fs.readFile(absPath, "utf8")
			const { contentHash } = await import("@/utils/line-hashing")
			const hash = contentHash(content)
			if (hash === entry.hash) {
				unchanged++
			} else {
				changed.push(relPath)
			}
		} catch {
			deleted.push(relPath)
		}
	}

	return { changed, deleted, unchanged }
}

export async function ensureStateDirectoryExists(): Promise<string> {
	return getGlobalStorageDir("state")
}

export async function ensureCacheDirectoryExists(): Promise<string> {
	return getGlobalStorageDir("cache")
}

async function getGlobalStorageDir(...subdirs: string[]) {
	const fullPath = path.resolve(HostProvider.get().globalStorageFsPath, ...subdirs)
	await fs.mkdir(fullPath, { recursive: true })
	return fullPath
}

export async function getTaskHistoryStateFilePath(): Promise<string> {
	return path.join(await ensureStateDirectoryExists(), "taskHistory.json")
}

export async function taskHistoryStateFileExists(): Promise<boolean> {
	const filePath = await getTaskHistoryStateFilePath()
	return fileExistsAtPath(filePath)
}

export async function readTaskHistoryFromState(): Promise<HistoryItem[]> {
	try {
		const filePath = await getTaskHistoryStateFilePath()
		if (!(await fileExistsAtPath(filePath))) {
			return []
		}

		const contents = await fs.readFile(filePath, "utf8")

		try {
			return JSON.parse(contents)
		} catch (parseError) {
			const result = await reconstructTaskHistory(false)
			if (result && result.reconstructedTasks > 0) {
				// Read the reconstructed file
				const newContents = await fs.readFile(filePath, "utf8")
				return JSON.parse(newContents)
			}

			// Recovery failed, all we can do is return an empty array or throw an error, thus preventing the app from starting up
			// This will wipe out the taskHistory
			return []
		}
	} catch (error) {
		// Filesystem or other errors - throw them for the caller to handle
		throw error
	}
}

export async function writeTaskHistoryToState(items: HistoryItem[]): Promise<void> {
	try {
		const filePath = await getTaskHistoryStateFilePath()
		await atomicWriteFile(filePath, JSON.stringify(items))
	} catch (error) {
		Logger.error("[Disk] Failed to write task history:", error)
		throw error
	}
}


/**
 * Workspace-local message history for CLI prompt history.
 * Stored at .dirac-state/message-history.json within each workspace root.
 */

const DIRAC_STATE_DIR = ".dirac-state"
const MESSAGE_HISTORY_FILE = "message-history.json"

export async function ensureWorkspaceStateDir(workspacePath: string): Promise<string> {
	const fullPath = path.resolve(workspacePath, DIRAC_STATE_DIR)
	await fs.mkdir(fullPath, { recursive: true })
	return fullPath
}

export async function getWorkspaceMessageHistoryPath(workspacePath: string): Promise<string> {
	const stateDir = await ensureWorkspaceStateDir(workspacePath)
	return path.join(stateDir, MESSAGE_HISTORY_FILE)
}

export async function readWorkspaceMessageHistory(workspacePath: string): Promise<string[]> {
	try {
		const filePath = await getWorkspaceMessageHistoryPath(workspacePath)
		if (!(await fileExistsAtPath(filePath))) {
			return []
		}
		const contents = await fs.readFile(filePath, "utf8")
		return JSON.parse(contents)
	} catch (error) {
		Logger.error("[Disk] Failed to read workspace message history:", error)
		return []
	}
}

export async function writeWorkspaceMessageHistory(workspacePath: string, messages: string[]): Promise<void> {
	try {
		const filePath = await getWorkspaceMessageHistoryPath(workspacePath)
		await atomicWriteFile(filePath, JSON.stringify(messages))
	} catch (error) {
		Logger.error("[Disk] Failed to write workspace message history:", error)
		throw error
	}
}
export async function readTaskSettingsFromStorage(taskId: string): Promise<Partial<Settings>> {
	try {
		const taskDirectoryFilePath = await ensureTaskDirectoryExists(taskId)
		const settingsFilePath = path.join(taskDirectoryFilePath, "settings.json")

		if (await fileExistsAtPath(settingsFilePath)) {
			const settingsContent = await fs.readFile(settingsFilePath, "utf8")
			return JSON.parse(settingsContent)
		}

		// Return empty object if settings file doesn't exist (new task)
		return {}
	} catch (error) {
		Logger.error("[Disk] Failed to read task settings:", error)
		throw error
	}
}

export async function writeTaskSettingsToStorage(taskId: string, settings: Partial<Settings>) {
	try {
		const taskDirectoryFilePath = await ensureTaskDirectoryExists(taskId)
		const settingsFilePath = path.join(taskDirectoryFilePath, "settings.json")

		let existingSettings = {}
		if (await fileExistsAtPath(settingsFilePath)) {
			const existingSettingsContent = await fs.readFile(settingsFilePath, "utf8")
			existingSettings = JSON.parse(existingSettingsContent)
		}

		const updatedSettings = { ...existingSettings, ...settings }
		await fs.writeFile(settingsFilePath, JSON.stringify(updatedSettings, null, 2))
	} catch (error) {
		Logger.error("[Disk] Failed to write task settings:", error)
		throw error
	}
}

export async function readRemoteConfigFromCache(organizationId: string): Promise<RemoteConfig | undefined> {
	try {
		const remoteConfigFilePath = path.join(await ensureCacheDirectoryExists(), GlobalFileNames.remoteConfig(organizationId))
		const fileExists = await fileExistsAtPath(remoteConfigFilePath)
		if (fileExists) {
			const fileContents = await fs.readFile(remoteConfigFilePath, "utf8")
			return JSON.parse(fileContents)
		}
		return undefined
	} catch (error) {
		Logger.error("Failed to read remote config from cache:", error)
		return undefined
	}
}

export async function writeRemoteConfigToCache(organizationId: string, config: RemoteConfig): Promise<void> {
	try {
		const remoteConfigFilePath = path.join(await ensureCacheDirectoryExists(), GlobalFileNames.remoteConfig(organizationId))
		await fs.writeFile(remoteConfigFilePath, JSON.stringify(config))
	} catch (error) {
		Logger.error("Failed to write remote config to cache:", error)
	}
}

export async function deleteRemoteConfigFromCache(organizationId: string): Promise<void> {
	try {
		const remoteConfigFilePath = path.join(await ensureCacheDirectoryExists(), GlobalFileNames.remoteConfig(organizationId))
		const fileExists = await fileExistsAtPath(remoteConfigFilePath)
		if (fileExists) {
			await fs.unlink(remoteConfigFilePath)
		}
	} catch (error) {
		Logger.error("Failed to delete remote config from cache:", error)
	}
}

/**
 * Gets the path to the global hooks directory if it exists.
 * Returns undefined if the directory doesn't exist.
 */
export async function getGlobalHooksDir(): Promise<string | undefined> {
	const globalHooksDir = await ensureHooksDirectoryExists()
	return (await isDirectory(globalHooksDir)) ? globalHooksDir : undefined
}

let runtimeHooksDir: string | undefined

/**
 * Sets a runtime hooks directory, typically passed via the --hooks-dir CLI flag.
 * This directory is included alongside global and workspace hooks directories
 * when discovering hooks.
 */
export function setRuntimeHooksDir(dir: string | undefined): void {
	runtimeHooksDir = dir
}

/**
 * Gets the paths to all hooks directories to search for hooks, including:
 * 1. The runtime hooks directory (if set via --hooks-dir CLI flag)
 * 2. The global hooks directory (if it exists)
 * 3. Each workspace root's .diracrules/hooks directory (if they exist)
 *
 * Note: Hooks from different directories may be executed concurrently.
 * No execution order is guaranteed between hooks from different directories.
 * A workspace may not use hooks, and the resulting array will be empty. A
 * multi-root workspace may have multiple hooks directories.
 */
export async function getAllHooksDirs(): Promise<string[]> {
	const hooksDirs: string[] = []

	// Add runtime hooks directory (set by --hooks-dir CLI flag)
	if (runtimeHooksDir && (await isDirectory(runtimeHooksDir))) {
		hooksDirs.push(runtimeHooksDir)
	}

	// Add global hooks directory (if it exists)
	const globalHooksDir = await getGlobalHooksDir()
	if (globalHooksDir) {
		hooksDirs.push(globalHooksDir)
	}

	// Add workspace hooks directories
	const workspaceHooksDirs = await getWorkspaceHooksDirs()
	hooksDirs.push(...workspaceHooksDirs)

	return hooksDirs
}

/**
 * Gets the paths to the workspace's .diracrules/hooks directories to search for
 * hooks. A workspace may not use hooks, and the resulting array will be empty. A
 * multi-root workspace may have multiple hooks directories.
 */
export async function getWorkspaceHooksDirs(): Promise<string[]> {
	const workspaceRootPaths =
		StateManager.get()
			.getGlobalStateKey("workspaceRoots")
			?.map((root) => root.path) || []

	return (
		await Promise.all(
			workspaceRootPaths.map(async (workspaceRootPath) => {
				// Look for a .diracrules/hooks folder in this workspace root.
				const candidate = path.join(workspaceRootPath, GlobalFileNames.hooksDir)
				return (await isDirectory(candidate)) ? candidate : undefined
			}),
		)
	).filter((path): path is string => Boolean(path))
}

/**
 * Writes the conversation history to a temporary JSON file for PreCompact hook consumption.
 * The file is created in the task's directory with a unique timestamp-based name.
 * Returns the absolute path to the created file.
 *
 * @param taskId The task ID
 * @param apiConversationHistory The conversation history to write
 * @param timestamp Optional timestamp to use for the filename (defaults to Date.now())
 * @returns The absolute path to the temporary file
 */
export async function writeConversationHistoryJson(
	taskId: string,
	apiConversationHistory: Anthropic.MessageParam[],
	timestamp?: number,
): Promise<string> {
	const taskDir = await ensureTaskDirectoryExists(taskId)
	const fileTimestamp = timestamp ?? Date.now()
	const tempFileName = `conversation_history_${fileTimestamp}.json`
	const tempFilePath = path.join(taskDir, tempFileName)

	try {
		await atomicWriteFile(tempFilePath, JSON.stringify(apiConversationHistory, null, 2))
		return tempFilePath
	} catch (error) {
		Logger.error("Failed to write conversation history JSON for hook:", error)
		throw error
	}
}

/**
 * Cleans up a temporary conversation history file created for hook execution.
 * Silently handles errors (file already deleted, permissions, etc.)
 *
 * @param filePath The path to the temporary file to delete
 */
export async function cleanupConversationHistoryFile(filePath: string): Promise<void> {
	try {
		if (await fileExistsAtPath(filePath)) {
			await fs.unlink(filePath)
		}
	} catch (error) {
		// Silently handle errors - this is cleanup, not critical
		Logger.debug("Failed to cleanup conversation history file:", filePath, error)
	}
}

/**
 * Writes the conversation history in human-readable text format to a temporary file for PreCompact hook consumption.
 * This formats the conversation history (user and assistant messages) in a readable text format,
 * making it easy to analyze the conversation flow without parsing JSON.
 *
 * @param taskId The task ID
 * @param conversationHistory The conversation history messages
 * @param timestamp Optional timestamp to use for the filename (defaults to Date.now())
 * @returns The absolute path to the temporary file
 */
export async function writeConversationHistoryText(
	taskId: string,
	conversationHistory: Anthropic.MessageParam[],
	timestamp?: number,
): Promise<string> {
	const taskDir = await ensureTaskDirectoryExists(taskId)
	const fileTimestamp = timestamp ?? Date.now()
	const tempFileName = `conversation_history_${fileTimestamp}.txt`
	const tempFilePath = path.join(taskDir, tempFileName)

	try {
		// Build the formatted conversation history (excluding system prompt)
		let fullContext = "=== CONVERSATION HISTORY ===\n\n"

		// Format each message in the conversation
		for (let i = 0; i < conversationHistory.length; i++) {
			const message = conversationHistory[i]
			fullContext += `--- Message ${i + 1} (${message.role.toUpperCase()}) ---\n`

			// Handle content which can be a string or array
			if (typeof message.content === "string") {
				fullContext += message.content
			} else if (Array.isArray(message.content)) {
				for (const block of message.content) {
					if (block.type === "text") {
						fullContext += block.text
					} else if (block.type === "image") {
						fullContext += `[IMAGE: ${block.source?.type || "unknown"}]`
					} else if (block.type === "tool_use") {
						fullContext += `[TOOL USE: ${block.name}]\n`
						fullContext += `Input: ${JSON.stringify(block.input, null, 2)}`
					} else if (block.type === "tool_result") {
						fullContext += `[TOOL RESULT: ${block.tool_use_id}]\n`
						if (typeof block.content === "string") {
							fullContext += block.content
						} else if (Array.isArray(block.content)) {
							for (const resultBlock of block.content) {
								if (resultBlock.type === "text") {
									fullContext += resultBlock.text
								} else if (resultBlock.type === "image") {
									fullContext += `[IMAGE]`
								}
							}
						}
					}
					fullContext += "\n\n"
				}
			}

			fullContext += "\n"
		}

		fullContext += "=== END OF CONTEXT ===\n"

		await atomicWriteFile(tempFilePath, fullContext)
		return tempFilePath
	} catch (error) {
		Logger.error("Failed to write conversation history text for hook:", error)
		throw error
	}
}
