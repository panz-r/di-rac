import type { ToolUse } from "@core/assistant-message"
import type { ToolResponse } from "./index"
import { DiracDefaultTool } from "@/shared/tools"

// --- Taxonomy ---

export enum ErrorDomain {
	SYSTEM = "SYSTEM",     // infrastructure: rate limits, timeouts, file locks
	ACTION = "ACTION",     // parameter errors, tool misuse, wrong format
	MEMORY = "MEMORY",     // retrieval miss, wrong file, stale reference
	PLANNING = "PLANNING", // requires LLM re-plan (no deterministic fix)
}

export enum ErrorCategory {
	TRANSIENT = "TRANSIENT", // Retry with backoff
	PERMANENT = "PERMANENT", // Never retry, escalate immediately
	EXECUTION = "EXECUTION", // Tool ran but produced wrong result
}

export type RecoveryTier = "transient" | "input_error" | "recoverable_logic" | "fatal"

export interface RecoveryEntry {
	domain: ErrorDomain
	category: ErrorCategory
	tier: RecoveryTier
	maxRetries: number
	cooldownMs?: number
	handler: (
		toolName: string,
		input: unknown,
		error: any, // or strongly typed error response
		attempt: number,
		executeUnderlying: (name: string, args: unknown) => Promise<ToolResponse>
	) => Promise<ToolResponse | null> // null = pass through to LLM for L3 Escalation
}

// --- State Tracking Types ---

export interface FailureRecord {
	errorCode: string
	tool: string
	successCount: number
	failureCount: number
	lastSeen: number
	permanentSkip?: boolean // Phase 7: Correction Learning
}

export interface CircuitState {
	state: "CLOSED" | "OPEN" | "HALF_OPEN"
	failures: number
	lastFailureTime: number
}

export interface CallRecord {
	tool: string
	fingerprint: string
	timestamp: number
}

export interface StagnationResult {
	stagnationDetected: boolean
	summary: string
}

// --- The Recovery Engine ---

export class RecoveryEngine {
	private recoveryTable: Record<string, RecoveryEntry> = {}
	private failureMemory = new Map<string, FailureRecord>()
	private globalFailureMemory = new Map<string, FailureRecord>() // Phase 15: Global Playbook
	private circuitBreakers = new Map<string, CircuitState>()
	private callHistory: CallRecord[] = []
	private perTurnTokenBudget: number = 5 // max 5 recovery retries per turn
	private currentTurnRetries: number = 0

	// Phase 3: Telemetry and Audit
	private telemetry = {
		interceptedCount: 0,
		escalatedCount: 0,
		totalTurnSavings: 0,
	}
	private lastAuditHash: string = "INIT"

	// Phase 5: Advanced Ledger and Session Skips
	private sessionSkips = new Set<string>()

	// Phase 11: Proactive Safeguards
	private completionAttemptCount: number = 0
	private stagnationHintCount: number = 0

	constructor() {
		this.initializeRecoveryTable()
		this.loadRecoveryMemory()
	}

	private async getGlobalPlaybookPath(): Promise<string> {
		const os = await import("os")
		const path = await import("path")
		return path.join(os.homedir(), ".dirac", "recovery-playbook.json")
	}

	private async loadRecoveryMemory() {
		const THIRTY_DAYS_MS = 30 * 24 * 60 * 60 * 1000

		// Helper to load a memory file into a map
		const loadToMap = async (filePath: string, targetMap: Map<string, FailureRecord>) => {
			try {
				const fs = await import("fs/promises")
				const content = await fs.readFile(filePath, "utf-8")
				const data = JSON.parse(content)
				Object.entries(data).forEach(([key, value]) => {
					const record = value as FailureRecord
					// Weight decay: if not seen in 30 days, demote (reduce success count)
					if (Date.now() - record.lastSeen > THIRTY_DAYS_MS && record.successCount >= 3) {
						record.successCount = 2 // Demote from graduated
					}
					targetMap.set(key, record)
				})
			} catch (e) {
				// Ignore if file doesn't exist or is malformed
			}
		}

		try {
			const path = await import("path")
			const localMemoryFile = path.join(process.cwd(), ".dirac-state", "recovery-memory.json")
			const globalMemoryFile = await this.getGlobalPlaybookPath()

			await loadToMap(localMemoryFile, this.failureMemory)
			await loadToMap(globalMemoryFile, this.globalFailureMemory)
		} catch (e) {
			// Ignore
		}
	}

	private async saveTelemetrySummary() {
		try {
			const fs = await import("fs/promises")
			const path = await import("path")
			const diracStateDir = path.join(process.cwd(), ".dirac-state")
			await fs.mkdir(diracStateDir, { recursive: true })
			const summaryFile = path.join(diracStateDir, "recovery-summary.json")
			const recoveryRate = this.telemetry.interceptedCount + this.telemetry.escalatedCount > 0
				? (this.telemetry.interceptedCount / (this.telemetry.interceptedCount + this.telemetry.escalatedCount)) * 100
				: 0
			
			const summary = {
				...this.telemetry,
				recoveryRate: `${recoveryRate.toFixed(1)}%`,
				timestamp: new Date().toISOString()
			}
			await fs.writeFile(summaryFile, JSON.stringify(summary, null, 2))
		} catch (e) {
			// Ignore errors
		}
	}

	private async updateAuditChain(toolName: string, errorCode: string, outcome: string) {
		try {
			const { createHash } = await import("node:crypto")
			const fs = await import("fs/promises")
			const path = await import("path")
			
			const data = `${this.lastAuditHash}:${toolName}:${errorCode}:${outcome}`
			this.lastAuditHash = createHash("sha256").update(data).digest("hex")
			
			const diracStateDir = path.join(process.cwd(), ".dirac-state")
			await fs.mkdir(diracStateDir, { recursive: true })
			const auditFile = path.join(diracStateDir, "recovery-audit.log")
			await fs.appendFile(auditFile, `${new Date().toISOString()} [${outcome}] ${toolName} (${errorCode}) -> ${this.lastAuditHash}\n`)
		} catch (e) {
			// Silent fail
		}
	}

	private async logToLedger(errorCode: string, toolName: string, action: string, success: boolean, args: unknown) {
		try {
			const fs = await import("fs/promises")
			const path = await import("path")
			const diracStateDir = path.join(process.cwd(), ".dirac-state")
			await fs.mkdir(diracStateDir, { recursive: true })
			const ledgerFile = path.join(diracStateDir, "recovery-ledger.jsonl")
			
			// Simple context fingerprint: stringify core args
			let contextFingerprint = ""
			try {
				contextFingerprint = this.fingerprintToolCall(toolName, args)
			} catch {
				contextFingerprint = "unknown"
			}

			const record = JSON.stringify({
				errorCode,
				tool: toolName,
				recoveryAction: action,
				success,
				contextFingerprint,
				timestamp: Date.now()
			}) + "\n"
			await fs.appendFile(ledgerFile, record)
		} catch (e) {
			// Silent fail
		}
	}

	private async saveRecoveryMemory() {
		const fs = await import("fs/promises")
		const path = await import("path")

		// Helper to save a map to a file
		const saveMapToFile = async (filePath: string, sourceMap: Map<string, FailureRecord>) => {
			try {
				const dir = path.dirname(filePath)
				await fs.mkdir(dir, { recursive: true })
				const data = Object.fromEntries(sourceMap.entries())
				await fs.writeFile(filePath, JSON.stringify(data, null, 2))
			} catch (e) {
				// Ignore
			}
		}

		try {
			const localMemoryFile = path.join(process.cwd(), ".dirac-state", "recovery-memory.json")
			const globalMemoryFile = await this.getGlobalPlaybookPath()

			await saveMapToFile(localMemoryFile, this.failureMemory)
			await saveMapToFile(globalMemoryFile, this.globalFailureMemory)
		} catch (e) {
			// Ignore errors
		}
	}

	public resetTurnBudget() {
		this.currentTurnRetries = 0
		this.stagnationHintCount = 0 // Phase 15 Hardening: Reset hints on new turn
	}

	public getTelemetry() {
		return {
			...this.telemetry,
			recoveryRate: this.telemetry.interceptedCount + this.telemetry.escalatedCount > 0
				? `${((this.telemetry.interceptedCount / (this.telemetry.interceptedCount + this.telemetry.escalatedCount)) * 100).toFixed(1)}%`
				: "0%"
		}
	}

	/**
	 * AEGIS-inspired Pre-Execution Firewall.
	 * Interposes on the tool-execution path to stop errors before they happen.
	 */
	public async runPreflightFirewall(
		block: ToolUse,
		taskState: any,
		dispatch: (name: string, args: unknown) => Promise<ToolResponse>
	): Promise<ToolResponse | null> {
		const toolName = block.name
		const params = block.params as any

		// --- Stage I: Argument Extraction ---
		let rawPath = params?.path || params?.file_path || params?.absolutePath
		
		// Phase 10+: Extract path from symbol handle if needed
		if (!rawPath && params?.handle && typeof params.handle === "string") {
			rawPath = params.handle.split(":")[0]
		}

		const startLine = params?.start_line
		const endLine = params?.end_line

		// Standardize to absolute path for tracking (Phase 15 refinement)
		let filePath = rawPath
		if (rawPath && typeof rawPath === "string") {
			try {
				const path = await import("path")
				
				// Phase 15 Hardening: Silently redirect /tmp to .dirac-tmp in project root
				if (rawPath.startsWith("/tmp")) {
					const relativeFromTmp = rawPath.slice(4).replace(/^[\/\\]+/, "") // remove leading slash
					const redirectedPath = path.join(".dirac-tmp", relativeFromTmp)
					
					// Mutate the original block params for silent redirection
					if (params.path) params.path = redirectedPath
					if (params.file_path) params.file_path = redirectedPath
					if (params.absolutePath) params.absolutePath = redirectedPath
					
					this.updateAuditChain(toolName, "PATH_REDIRECTED", "SILENT_FIX")
					rawPath = redirectedPath
				}

				// Phase 15 Hardening: Silently redirect /tmp in BASH commands
				if (toolName === DiracDefaultTool.BASH && params.command && typeof params.command === "string") {
					if (params.command.includes("/tmp")) {
						params.command = params.command.replace(/\/tmp\b/g, ".dirac-tmp")
						this.updateAuditChain(toolName, "CMD_REDIRECTED", "SILENT_FIX")
					}
				}

				filePath = path.isAbsolute(rawPath) ? rawPath : path.resolve(process.cwd(), rawPath)
			} catch {
				// Fallback to raw if path module fails
			}
		}

		// --- Stage II: Content & State Scanning ---
		
		// 0. Phase and Token Tracking (Phase 5)
		if (taskState) {
			const mutationTools = [DiracDefaultTool.EDIT_FILE, DiracDefaultTool.FILE_NEW, DiracDefaultTool.BASH]
			const verificationTools = [DiracDefaultTool.BASH_RESTRICTED, DiracDefaultTool.DIAGNOSTICS_SCAN]
			
			if (mutationTools.includes(toolName as any)) {
				if (taskState.currentTaskPhase === "exploration") {
					taskState.currentTaskPhase = "editing"
				}
				if (toolName === DiracDefaultTool.BASH) {
					taskState.didExecuteCommand = true
				}
			} else if (verificationTools.includes(toolName as any) && taskState.currentTaskPhase === "editing") {
				taskState.currentTaskPhase = "verification"
			}

			// Token Efficiency Heuristic
			if (toolName === DiracDefaultTool.FILE_READ && typeof startLine === "number" && typeof endLine === "number") {
				const lineCount = endLine - startLine
				taskState.turnTokenEstimates += lineCount * 50 // Very rough estimate
				if (taskState.turnTokenEstimates > 5000 && taskState.currentTaskPhase === "exploration") {
					this.updateAuditChain(toolName, "LOW_EFFICIENCY_READ", "WARNING")
					// We don't block here, but detectStagnation will use this
				}
			}
		}

		// Phase 11: Premature Success Detection
		if (toolName === DiracDefaultTool.ATTEMPT) {
			const hasMadeChanges = taskState.didEditFile || taskState.didExecuteCommand
			if (!hasMadeChanges && this.completionAttemptCount === 0) {
				this.completionAttemptCount++
				this.updateAuditChain(toolName, "PREMATURE_COMPLETION", "BLOCKED")
				return this.formatStructuredEscalation(
					toolName,
					block.params,
					"PREMATURE_COMPLETION",
					"You are attempting to complete the task without having made any modifications or running any verification commands.",
					"If the task requires changes, please perform them before finishing. If this is intentional, you may proceed by calling attempt_completion again."
				)
			}
			// Reset or allow if it's the second attempt or changes were made
			this.completionAttemptCount = 0
		}

		// 1. Paradoxical Ranges Check
		if (toolName === DiracDefaultTool.FILE_READ && typeof startLine === "number" && typeof endLine === "number") {
			if (startLine > endLine) {
				// Stage III Policy: Silent Fix
				block.params.start_line = endLine
				block.params.end_line = startLine
				this.updateAuditChain(toolName, "PARADOXICAL_RANGE", "SILENT_FIX")
			}
		}

		// 2. Stale Context Check (Phase 8: Block -> Phase 15 Hardening: Silent Re-read)
		if (toolName === DiracDefaultTool.EDIT_FILE && typeof filePath === "string") {
			const lastAccess = taskState.fileLastAccessToolIndex.get(filePath)
			const currentCount = taskState.totalToolCallCount
			
			if (lastAccess !== undefined && (currentCount - lastAccess) > 15) {
				// Stage III Policy: Silent Context Refresh
				this.updateAuditChain(toolName, "STALE_CONTEXT", "SILENT_REFRESH")
				await dispatch(DiracDefaultTool.FILE_READ, {
					path: filePath,
					detail: "outline"
				})
				// Success! Context refreshed. Proceeding to execution.
			}
		}

		// 3. Overlapping Edit Check (Phase 8: Hard Block)
		if (toolName === DiracDefaultTool.EDIT_FILE && typeof filePath === "string") {
			if (taskState.filesEditedInCurrentTurn.has(filePath)) {
				// Stage III Policy: Overlapping Edit Block
				this.updateAuditChain(toolName, "OVERLAPPING_EDIT", "BLOCKED")
				return this.formatStructuredEscalation(
					toolName,
					block.params,
					"OVERLAPPING_EDIT",
					`You have already edited '${filePath}' in this turn.`,
					`To prevent conflicting changes and line-number drift, please consolidate your edits into a single tool call or wait for the previous edit to apply.`
				)
			}
		}

		// Phase 12: Heuristic Edit Repair
		if (toolName === DiracDefaultTool.EDIT_FILE && params?.edits) {
			const edits = params.edits as any[]
			let repaired = false
			for (const edit of edits) {
				if (edit.text && typeof edit.text === "string") {
					const text = edit.text
					// Detect common malformed block: missing ======= but has markers
					if (text.includes("<<<<<<< SEARCH") && text.includes(">>>>>>> REPLACE") && !text.includes("=======")) {
						// Heuristically inject separator (very basic implementation)
						const lines = text.split("\n")
						const searchIdx = lines.findIndex((l: string) => l.includes("<<<<<<< SEARCH"))
						const replaceIdx = lines.findIndex((l: string) => l.includes(">>>>>>> REPLACE"))
						if (searchIdx !== -1 && replaceIdx !== -1 && replaceIdx > searchIdx + 1) {
							// For now, let's not auto-inject if it's ambiguous. 
							// But we can normalize case.
						}
					}
					// Normalize marker case
					if (text.includes("<<<<<<< search") || text.includes(">>>>>>> replace")) {
						edit.text = text
							.replace(/<<<<<<< search/gi, "<<<<<<< SEARCH")
							.replace(/>>>>>>> replace/gi, ">>>>>>> REPLACE")
							.replace(/=======/gi, "=======")
						repaired = true
					}
				}
			}
			if (repaired) {
				this.updateAuditChain(toolName, "MALFORMED_EDIT", "SILENT_FIX")
			}
		}

		// 4. Symbol Freshness Check (Phase 8: Pre-flight -> Phase 10: Silent Re-parse)
		if (toolName === DiracDefaultTool.EXPAND_SYMBOL && typeof filePath === "string") {
			try {
				const fs = await import("fs/promises")
				const stats = await fs.stat(filePath)
				const mtime = stats.mtimeMs
				const lastIndexedMtime = taskState.symbolIndexMtimes.get(filePath)
				
				if (lastIndexedMtime !== undefined && mtime > lastIndexedMtime) {
					// Phase 10: Silent Re-parse instead of Block
					this.updateAuditChain(toolName, "STALE_SYMBOL_INDEX", "SILENT_REPARSE")
					await dispatch(DiracDefaultTool.FILE_READ, {
						path: filePath,
						detail: "outline"
					})
					// Success! The index is now refreshed. Proceeding to execution.
				}
			} catch {
				// Ignore stat errors in pre-flight
			}
		}

		// Update tracking state (AEGIS Stage I/II side effect)
		if (filePath && typeof filePath === "string") {
			taskState.fileLastAccessToolIndex.set(filePath, taskState.totalToolCallCount)
			taskState.filesTouchedInCurrentTurn.add(filePath)

			// Phase 8: Track edit state for overlapping detection
			if (toolName === DiracDefaultTool.EDIT_FILE || toolName === DiracDefaultTool.FILE_NEW) {
				taskState.filesEditedInCurrentTurn.add(filePath)
			}

			// Phase 8: Track symbol freshness
			if (toolName === DiracDefaultTool.SEARCH_SYMBOLS) {
				// Global search operation - for simplicity, we mark all current indexed files as potentially refreshed
				// or better, we let the individual expand_symbol check handle it.
				// For now, clear the mtime map to force re-validation on next expand.
				taskState.symbolIndexMtimes.clear()
			} else if (toolName === DiracDefaultTool.FILE_READ && params?.detail === "outline") {
				try {
					const fs = await import("fs/promises")
					const stats = await fs.stat(filePath)
					taskState.symbolIndexMtimes.set(filePath, stats.mtimeMs)
				} catch {
					taskState.symbolIndexMtimes.set(filePath, Date.now())
				}
			}
		}

		return null // Pass through to execution
	}

	private initializeRecoveryTable() {
		this.recoveryTable = {
			// --- SYSTEM DOMAIN ---
			FILE_LOCKED: {
				domain: ErrorDomain.SYSTEM,
				category: ErrorCategory.TRANSIENT,
				tier: "transient",
				maxRetries: 1,
				cooldownMs: 100,
				handler: async (toolName, input, _error, _attempt, execute) => {
					await new Promise((r) => setTimeout(r, 100))
					return execute(toolName, input)
				},
			},
			LSP_TIMEOUT: {
				domain: ErrorDomain.SYSTEM,
				category: ErrorCategory.TRANSIENT,
				tier: "recoverable_logic",
				maxRetries: 1,
				handler: async (toolName, input: any, _error, _attempt, execute) => {
					return execute(toolName, { ...input, useLsp: false })
				},
			},
			RATE_LIMITED: {
				domain: ErrorDomain.SYSTEM,
				category: ErrorCategory.TRANSIENT,
				tier: "transient",
				maxRetries: 2,
				cooldownMs: 1000,
				handler: async (toolName, input, _error, _attempt, execute) => {
					await new Promise((r) => setTimeout(r, 1000))
					return execute(toolName, input)
				},
			},

			// --- ACTION DOMAIN ---
			ANCHOR_NOT_FOUND: {
				domain: ErrorDomain.ACTION,
				category: ErrorCategory.PERMANENT,
				tier: "recoverable_logic",
				maxRetries: 1,
				handler: async (toolName, input: any, error, _attempt, execute) => {
					// Re-read file outline to refresh symbol/line map
					const filePath = input.file_path || input.path
					if (!filePath) return null

					const outlineResult = await execute(DiracDefaultTool.FILE_READ, {
						path: filePath,
						detail: "outline"
					})

					// Phase 7: Autonomous Anchor Re-mapping
					const outlineText = this.extractErrorMessage(outlineResult)
					
					// Try to find the symbol handle in the original params
					// If LLM used a symbol-based anchor like "fn:login", we can find its new line
					const anchor = input.anchor
					if (anchor && typeof anchor === "string") {
						// Look for [anchor] in the outline using regex
						// Format:   - [anchor] signature (lines start-end)
						const anchorEscaped = anchor.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
						const lineMatch = outlineText.match(new RegExp(`\\[${anchorEscaped}\\] .* \\(lines (\\d+)-(\\d+)\\)`))
						if (lineMatch) {
							const newLine = Number.parseInt(lineMatch[1])
							const newInput = { ...input, line: newLine }
							// Success! Re-execute the edit with the corrected line
							this.updateAuditChain(toolName, "ANCHOR_REMAPPED", "SILENT_FIX")
							return await execute(toolName as any, newInput)
						}
					}

					return outlineResult
				},
			},
			UNKNOWN_FLAG: {
				domain: ErrorDomain.ACTION,
				category: ErrorCategory.PERMANENT,
				tier: "input_error",
				maxRetries: 0,
				handler: async (_toolName, _input, _error, _attempt, _execute) => {
					return null // pass through -- LLM needs --help
				},
			},

			// --- MEMORY DOMAIN ---
			SYMBOL_NOT_FOUND: {
				domain: ErrorDomain.MEMORY,
				category: ErrorCategory.PERMANENT,
				tier: "recoverable_logic",
				maxRetries: 1,
				handler: async (toolName, input: any, _error, _attempt, execute) => {
					const symbolName = input.symbol || input.name
					if (!symbolName) return null

					// Re-run search_symbols to find the new handle
					const searchResult = await execute(DiracDefaultTool.SEARCH_SYMBOLS, {
						query: symbolName
					})

					// Phase 7: One-Hit Expansion
					const resultText = this.extractErrorMessage(searchResult)
					const matchCountMatch = resultText.match(/Found (\d+) matching symbols/)
					if (matchCountMatch && matchCountMatch[1] === "1") {
						const handleMatch = resultText.match(/\[([^\]]+)\]/)
						if (handleMatch) {
							const handle = handleMatch[1]
							this.updateAuditChain(toolName, "SYMBOL_AUTO_EXPANDED", "SILENT_FIX")
							return await execute(DiracDefaultTool.EXPAND_SYMBOL, {
								handle
							})
						}
					}

					return searchResult
				},
			},
			FILE_CHANGED_SINCE_READ: {
				domain: ErrorDomain.MEMORY,
				category: ErrorCategory.PERMANENT,
				tier: "recoverable_logic",
				maxRetries: 1,
				handler: async (toolName, input: any, _error, _attempt, execute) => {
					const filePath = input.path || input.file_path
					if (!filePath) return null

					// Phase 10: Contextual Recovery (PALADIN style)
					const outlineResult = await execute(DiracDefaultTool.FILE_READ, {
						path: filePath,
						detail: "outline"
					})

					if (Array.isArray(outlineResult)) {
						// Phase 12: Diff-aware hint
						const hint = `[SYSTEM: CONTEXTUAL_RECOVERY] The file has changed since your last read. I have fetched the updated structural outline below. 
Some line numbers may have shifted. Please locate your target function in the new outline and issue a new edit command with the updated line numbers.`
						const textBlock = outlineResult.find(b => b.type === "text")
						if (textBlock && (textBlock as any).text) {
							(textBlock as any).text = `${hint}\n\n${(textBlock as any).text}`
						}
					}

					return outlineResult
				},
			},
			PathEscapeError: {
				domain: ErrorDomain.ACTION,
				category: ErrorCategory.PERMANENT,
				tier: "recoverable_logic",
				maxRetries: 1,
				handler: async (toolName, input: any, error, _attempt, execute) => {
					// Handle cases where the LLM provides an absolute path that is inside the workspace
					const rawPath = input.path || input.file_path || input.absolutePath
					if (!rawPath || typeof rawPath !== "string") return null

					try {
						const path = await import("path")
						const fs = await import("fs/promises")
						const workspaceRoot = process.cwd()

						// Standardize the path
						const resolvedPath = path.resolve(workspaceRoot, rawPath)

						// If the path starts with the workspace root, it's a safe absolute path that can be made relative
						if (resolvedPath.startsWith(workspaceRoot + path.sep)) {
							const relativePath = path.relative(workspaceRoot, resolvedPath)
							const newInput = { ...input }

							// Update whatever path parameter was used
							if (input.path) newInput.path = relativePath
							if (input.file_path) newInput.file_path = relativePath
							if (input.absolutePath) newInput.absolutePath = relativePath

							// Success! Retrying with relative path.
							this.updateAuditChain(toolName, "PATH_REWRITTEN", "SILENT_FIX")
							return await execute(toolName as any, newInput)
						}

						// Phase 15 Hardening: Provide specific hints for common absolute paths like /tmp
						if (rawPath.startsWith("/tmp")) {
							return this.formatStructuredEscalation(
								toolName,
								input,
								"PATH_ESCAPE",
								`You attempted to access '${rawPath}', which is outside the workspace.`,
								"Please use a project-relative path like '.dirac-tmp/' or '.tmp/' instead. Absolute paths are forbidden."
							)
						}
					} catch (e) {
						// Fall through to escalation
					}

					// Provide a strong general hint for any other escape
					return this.formatStructuredEscalation(
						toolName,
						input,
						"PATH_ESCAPE",
						`Path escape detected: '${rawPath}' is outside the workspace root.`,
						"Always use project-relative paths (e.g. 'src/main.ts') instead of absolute paths."
					)
				},
			},
			DIRAC_IGNORE_BLOCK: {
				domain: ErrorDomain.ACTION,
				category: ErrorCategory.PERMANENT,
				tier: "input_error",
				maxRetries: 0,
				handler: async (toolName, input: any, _error, _attempt, _execute) => {
					const rawPath = input.path || input.file_path || input.absolutePath
					return this.formatStructuredEscalation(
						toolName,
						input,
						"DIRAC_IGNORE_BLOCK",
						`Access to '${rawPath}' is blocked by the .diracignore file settings.`,
						"If you MUST access this file, ask the user to update the .diracignore file. Otherwise, please find another way to proceed without using this file."
					)
				}
			},
			EISDIR: {
				domain: ErrorDomain.MEMORY,
				category: ErrorCategory.PERMANENT,
				tier: "recoverable_logic",
				maxRetries: 1,
				handler: async (_toolName, input: any, _error, _attempt, execute) => {
					const rawPath = input.path || input.file_path || input.absolutePath
					if (!rawPath) return null

					// LLM tried to read/edit a directory. Help it by listing contents.
					this.updateAuditChain("EISDIR", "DIRECTORY_LISTED", "SILENT_FIX")
					return await execute(DiracDefaultTool.LIST_FILES, {
						path: rawPath,
						recursive: false
					})
				},
			},
			FILE_NOT_FOUND: {
				domain: ErrorDomain.MEMORY,
				category: ErrorCategory.PERMANENT,
				tier: "input_error",
				maxRetries: 1,
				handler: async (toolName, input: any, _error, _attempt, execute) => {
					const rawPath = input.path || input.file_path || input.absolutePath
					if (!rawPath) return null

					try {
						const path = await import("path")
						const fs = await import("fs/promises")
						const parentPath = path.dirname(rawPath)

						// Phase 13: Resource Acquisition (mkdir -p for writes)
						if (toolName === DiracDefaultTool.FILE_NEW || toolName === DiracDefaultTool.EDIT_FILE) {
							this.updateAuditChain(toolName, "RESOURCE_ACQUIRED", "SILENT_FIX")
							await fs.mkdir(parentPath, { recursive: true })
							
							// Retry the original write/edit call
							const result = await execute(toolName as any, input)
							
							if (Array.isArray(result)) {
								const hint = `[SYSTEM: RESOURCE_ACQUIRED - COMPACTION_SAFE] The parent directory for '${rawPath}' did not exist. I have created it for you.`
								const textBlock = result.find(b => b.type === "text")
								if (textBlock && (textBlock as any).text) {
									(textBlock as any).text = `${hint}\n\n${(textBlock as any).text}`
								}
							}
							return result
						}
						
						// List parent directory to find typos or missing levels (Original recovery)
						this.updateAuditChain("FILE_NOT_FOUND", "PARENT_LISTED", "SILENT_FIX")
						return await execute(DiracDefaultTool.LIST_FILES, {
							path: parentPath,
							recursive: false
						})
					} catch {
						return null
					}
				},
			},
			ENOTDIR: {
				domain: ErrorDomain.MEMORY,
				category: ErrorCategory.PERMANENT,
				tier: "recoverable_logic",
				maxRetries: 1,
				handler: async (_toolName, input: any, _error, _attempt, execute) => {
					const rawPath = input.path || input.file_path || input.absolutePath
					if (!rawPath) return null

					try {
						const path = await import("path")
						const parentPath = path.dirname(rawPath)
						this.updateAuditChain("ENOTDIR", "PARENT_LISTED", "SILENT_FIX")
						return await execute(DiracDefaultTool.LIST_FILES, {
							path: parentPath,
							recursive: false
						})
					} catch {
						return null
					}
				},
			},
			MISSING_PARAMETER: {
				domain: ErrorDomain.ACTION,
				category: ErrorCategory.PERMANENT,
				tier: "input_error",
				maxRetries: 0,
				handler: async (toolName, _input, _error, _attempt, _execute) => {
					// We can't deterministic fix a missing parameter, but we can provide a better L3 message.
					return null // Escalate with the heuristic message in handleErrorRecovery
				}
			},
			INVALID_ARGUMENT: {
				domain: ErrorDomain.ACTION,
				category: ErrorCategory.PERMANENT,
				tier: "input_error",
				maxRetries: 0,
				handler: async (toolName, _input, _error, _attempt, _execute) => {
					return null // Escalate with the heuristic message
				}
			},
			EMPTY_SEARCH_RESULTS: {
				domain: ErrorDomain.MEMORY,
				category: ErrorCategory.PERMANENT,
				tier: "recoverable_logic",
				maxRetries: 1,
				handler: async (toolName, input: any, _error, _attempt, execute) => {
					const query = input.query || input.regex
					if (!query || typeof query !== "string") return null

					// Phase 14: Search Broadening
					// If the query looks like a path, try broadening it to just the filename
					if (query.includes("/") || query.includes("\\")) {
						const path = await import("path")
						const broaderQuery = path.basename(query)
						if (broaderQuery !== query) {
							this.updateAuditChain(toolName, "SEARCH_BROADENED", "SILENT_FIX")
							const newInput = { ...input }
							if (input.query) newInput.query = broaderQuery
							if (input.regex) newInput.regex = broaderQuery
							
							const result = await execute(toolName as any, newInput)
							if (Array.isArray(result)) {
								const hint = `[SYSTEM: SEARCH_BROADENED - COMPACTION_SAFE] Your original search was too specific. I have broadened it to '${broaderQuery}' and found the following results.]`
								const textBlock = result.find(b => b.type === "text")
								if (textBlock && (textBlock as any).text) {
									(textBlock as any).text = `${hint}\n\n${(textBlock as any).text}`
								}
							}
							return result
						}
					}
					return null
				},
			},
			CONTEXT_OVERFLOW: {
				domain: ErrorDomain.SYSTEM,
				category: ErrorCategory.PERMANENT,
				tier: "recoverable_logic",
				maxRetries: 1,
				handler: async (toolName, input, _error, _attempt, execute) => {
					// Phase 14: Context Pressure Recovery
					this.updateAuditChain(toolName, "CONTEXT_PRESSURE", "SILENT_CONDENSE")
					
					// Trigger automatic condensation
					const condenseResult = await execute(DiracDefaultTool.CONDENSE, {})
					
					// If condensation succeeded (or even if it returned a summary), retry the original tool
					// We check if the result looks like a success or a summary
					return await execute(toolName as any, input)
				}
			}
		}
	}

	// --- Core Wrapper ---

	/**
	 * Wraps tool execution with the 3-level deterministic error recovery hierarchy.
	 */
	public async wrapWithRecovery(
		toolName: string,
		args: unknown,
		taskState: any,
		dispatch: (name: string, args: unknown) => Promise<ToolResponse>
	): Promise<ToolResponse> {
		// 1. Check Circuit Breaker
		const circuit = this.checkCircuit(toolName)
		if (circuit.state === "OPEN") {
			this.updateAuditChain(toolName, "CIRCUIT_BREAKER_OPEN", "BLOCKED")
			return this.formatStructuredEscalation(
				toolName,
				args,
				"CIRCUIT_BREAKER_OPEN",
				"Too many consecutive failures for this tool.",
				"Stop using this tool for a while or try a different approach."
			)
		}

		// 2. Check Stagnation (L2 Backtracking)
		const stagnation = this.detectStagnation(toolName, args, taskState)
		if (stagnation) {
			this.stagnationHintCount++
			
			// Hard block circuit breaker (after 3 consecutive hints)
			if (this.stagnationHintCount >= 3) {
				this.updateAuditChain(toolName, "STAGNATION_CIRCUIT_OPEN", "BLOCKED")
				return this.formatStructuredEscalation(
					toolName,
					args,
					"STAGNATION_LIMIT",
					`Loop protection triggered after 3 hints. ${stagnation.summary}`,
					"Please provide a higher-level summary of your goal or ask the user for a new direction."
				)
			}

			// L2 Nudge: Inject a structured hint into context, but do NOT block exploration
			this.updateAuditChain(toolName, "STAGNATION_HINT", "NUDGE")
			const isExactRepeat = stagnation.summary.includes("identical")
			const isAlternating = stagnation.summary.includes("Alternating")
			const isCircular = stagnation.summary.includes("Circular")
			
			let nextSteps = "You are repeating the same action. Please reconsider your approach or check the tool parameters."
			if (isCircular) {
				nextSteps = "Circular strategy loop detected. Please break the cycle, summarize what you've learned, and pivot to a new approach."
			} else if (isAlternating) {
				nextSteps = "You are alternating between tools without progress. Please pivot to a new strategy or summarize your current state."
			} else if (!isExactRepeat) {
				nextSteps = "You have been exploring without making progress. Consider switching to editing or refine your search."
			}

			const hintResponse = this.formatStructuredEscalation(
				toolName,
				args,
				"STAGNATION_DETECTED",
				stagnation.summary,
				nextSteps
			)

			// If it's a read-only tool, we can allow it to proceed BUT prepend the hint
			const readOnlyTools = [DiracDefaultTool.FILE_READ, DiracDefaultTool.LIST_FILES, DiracDefaultTool.SEARCH, DiracDefaultTool.GET_FUNCTION, DiracDefaultTool.EXPAND_SYMBOL]
			if (readOnlyTools.includes(toolName as any)) {
				const actualResult = await dispatch(toolName, args)
				if (Array.isArray(actualResult) && Array.isArray(hintResponse)) {
					return [...hintResponse, ...actualResult] as any
				}
				return hintResponse // Fallback to just hint if types mismatch
			}

			return hintResponse // Hard block for non-read tools
		}

		// Reset hint count on progress (non-stagnant call)
		this.stagnationHintCount = 0

		// Record the call for future stagnation checks
		this.recordCall(toolName, args)

		// 3. Execution
		try {
			const result = await dispatch(toolName, args)

			// Check if the tool returned an error (some tools return errors as formatted success responses for LLM)
			// This depends on how ToolResponse is structured. For now, assume we can extract errorCode.
			const errorCode = this.extractErrorCode(result)
			if (!errorCode) {
				this.updateCircuit(toolName, true)
				this.updateAuditChain(toolName, "NONE", "SUCCESS")
				return result // Success
			}

			// Tool returned a structured error
			return await this.handleErrorRecovery(toolName, args, errorCode, result, dispatch, taskState)

		} catch (error: any) {
			// Tool threw an exception
			const errorCode = error.code || error.name || "UNKNOWN_ERROR"
			return await this.handleErrorRecovery(toolName, args, errorCode, error, dispatch, taskState)
		}
	}

	// --- Recovery Logic (L1 & L3) ---

	private async logRecoveryMiss(
		errorCode: string,
		toolName: string,
		domain: string,
		failureCategory: string,
		attemptedRecovery: boolean,
		turnNumber: number
	) {
		try {
			const fs = await import("fs/promises")
			const path = await import("path")
			const diracStateDir = path.join(process.cwd(), ".dirac-state")
			await fs.mkdir(diracStateDir, { recursive: true })
			const logFile = path.join(diracStateDir, "recovery-misses.jsonl")
			const record = JSON.stringify({
				errorCode,
				tool: toolName,
				domain,
				failureCategory,
				attemptedRecovery,
				turnNumber,
				timestamp: Date.now(),
				recovered: false
			}) + "\n"
			await fs.appendFile(logFile, record)
		} catch (e) {
			// Silently fail logging to avoid disrupting the main loop
		}
	}

	private async handleErrorRecovery(
		toolName: string,
		args: unknown,
		errorCode: string,
		originalError: any,
		dispatch: (name: string, args: unknown) => Promise<ToolResponse>,
		taskState?: any
	): Promise<ToolResponse> {
		this.updateCircuit(toolName, false)
		const errorMessage = this.extractErrorMessage(originalError)
		const turnNumber = taskState?.totalToolCallCount || 0

		// Check if we should skip due to graduated failure memory
		if (this.shouldSkipRecovery(errorCode)) {
			this.telemetry.escalatedCount++
			this.saveTelemetrySummary()
			this.updateAuditChain(toolName, errorCode, "SKIPPED_GRADUATED")

			const skipReason = `Prior recovery attempts for ${errorCode} have consistently failed. Bypassing deterministic recovery.`
			this.logRecoveryMiss(
				errorCode,
				toolName,
				this.classifyErrorDomain(errorCode, errorMessage),
				this.classifyFailureCategory(errorCode, errorMessage),
				false,
				turnNumber
			)
			return this.formatStructuredEscalation(
				toolName,
				args,
				errorCode,
				skipReason,
				"Deterministic recovery is disabled for this repeating error. Please resolve it manually."
			)
		}

		const entry = this.recoveryTable[errorCode]
		if (!entry) {
			// No deterministic fix known -> L3 Escalation
			this.telemetry.escalatedCount++
			this.saveTelemetrySummary()
			this.updateAuditChain(toolName, errorCode, "UNHANDLED")

			this.logRecoveryMiss(
				errorCode,
				toolName,
				this.classifyErrorDomain(errorCode, errorMessage),
				this.classifyFailureCategory(errorCode, errorMessage),
				false,
				turnNumber
			)
			return this.formatStructuredEscalation(toolName, args, errorCode, errorMessage)
		}

		// Check Domain & Category heuristics
		if (entry.category === ErrorCategory.PERMANENT && entry.maxRetries === 0) {
			this.telemetry.escalatedCount++
			this.saveTelemetrySummary()
			this.updateAuditChain(toolName, errorCode, "PERMANENT_NO_RETRY")

			this.logRecoveryMiss(
				errorCode,
				toolName,
				entry.domain,
				this.classifyFailureCategory(errorCode, errorMessage),
				false,
				turnNumber
			)
			return this.formatStructuredEscalation(toolName, args, errorCode, errorMessage)
		}

		// Budget check
		if (this.currentTurnRetries >= this.perTurnTokenBudget) {
			this.telemetry.escalatedCount++
			this.saveTelemetrySummary()
			this.updateAuditChain(toolName, errorCode, "BUDGET_EXCEEDED")

			const budgetReason = "Recovery retry budget exceeded for this turn."
			this.logRecoveryMiss(
				errorCode,
				toolName,
				entry.domain,
				"Budget Exceeded",
				false,
				turnNumber
			)
			return this.formatStructuredEscalation(
				toolName,
				args,
				errorCode,
				budgetReason,
				"Too many automatic retries. Please reconsider your approach."
			)
		}

		// L1: Context Refinement (Execute Recovery Handler)
		this.currentTurnRetries++
		let recoveryResult: ToolResponse | null = null
		try {
			recoveryResult = await entry.handler(toolName, args, originalError, 1, dispatch)
		} catch (handlerError: any) {
			this.recordRecovery(errorCode, false)
			this.logToLedger(errorCode, toolName, "handler_crash", false, args)
			
			// Claude-smart rule: demote graduated pattern if it fails in this session
			const record = this.failureMemory.get(errorCode)
			if (record && record.successCount >= 3) {
				this.sessionSkips.add(errorCode)
			}

			this.telemetry.escalatedCount++
			this.saveTelemetrySummary()
			this.updateAuditChain(toolName, errorCode, "HANDLER_CRASH")

			const crashReason = `Recovery handler crashed for ${errorCode}.`
			this.logRecoveryMiss(
				errorCode,
				toolName,
				entry.domain,
				"Handler Crash",
				true,
				turnNumber
			)
			return this.formatStructuredEscalation(
				toolName,
				args,
				errorCode,
				crashReason,
				`Original error: ${errorMessage}`
			)
		}

		if (recoveryResult === null) {
			// Handler passed through to L3 Escalation
			this.recordRecovery(errorCode, false)
			this.logToLedger(errorCode, toolName, "handler_deferred", false, args)

			// Claude-smart rule: demote graduated pattern if it fails in this session
			const record = this.failureMemory.get(errorCode)
			if (record && record.successCount >= 3) {
				this.sessionSkips.add(errorCode)
			}

			this.telemetry.escalatedCount++
			this.saveTelemetrySummary()
			this.updateAuditChain(toolName, errorCode, "HANDLER_DEFERRED")

			const failReason = `Deterministic recovery failed or deferred for ${errorCode}.`
			this.logRecoveryMiss(
				errorCode,
				toolName,
				entry.domain,
				"Recovery Failed",
				true,
				turnNumber
			)
			return this.formatStructuredEscalation(
				toolName,
				args,
				errorCode,
				failReason,
				`Original error: ${errorMessage}`
			)
		}

		// Check if the recovery attempt itself returned an error
		const recoveryErrorCode = this.extractErrorCode(recoveryResult)
		if (recoveryErrorCode) {
			this.recordRecovery(errorCode, false)
			this.logToLedger(errorCode, toolName, `chain_failure:${recoveryErrorCode}`, false, args)

			// Claude-smart rule: demote graduated pattern if it fails in this session
			const record = this.failureMemory.get(errorCode)
			if (record && record.successCount >= 3) {
				this.sessionSkips.add(errorCode)
			}

			this.telemetry.escalatedCount++
			this.saveTelemetrySummary()
			this.updateAuditChain(toolName, errorCode, "CHAIN_FAILURE")

			const retryErrorReason = `Recovery attempt for ${errorCode} resulted in another error: ${recoveryErrorCode}.`
			this.logRecoveryMiss(
				errorCode,
				toolName,
				entry.domain,
				"Chain Failure",
				true,
				turnNumber
			)
			return this.formatStructuredEscalation(
				toolName,
				args,
				errorCode,
				retryErrorReason,
				`Original error: ${errorMessage}`
			)
		}

		// Recovery Success!
		this.recordRecovery(errorCode, true)
		this.logToLedger(errorCode, toolName, "success", true, args)

		this.telemetry.interceptedCount++
		this.telemetry.totalTurnSavings += 1.5
		this.saveTelemetrySummary()
		this.updateAuditChain(toolName, errorCode, "RECOVERED")

		// Mark as compaction-safe (L-ICL principle / Governance Integration)
		if (Array.isArray(recoveryResult)) {
			let successHint = "[SYSTEM: DETERMINISTIC_RECOVERY_SUCCESS - COMPACTION_SAFE]"
			if (errorCode === "ANCHOR_NOT_FOUND") {
				successHint = "[SYSTEM: ANCHOR_REMAPPED - COMPACTION_SAFE] Anchor stale due to file modifications. I found the target and applied your change. Refer to refreshed line numbers in future calls."
			} else if (errorCode === "PathEscapeError" || errorCode === "PATH_REWRITTEN") {
				successHint = "[SYSTEM: PATH_REWRITTEN - COMPACTION_SAFE] Absolute path converted to project-relative. Please use relative paths in the future."
			}

			recoveryResult.push({
				type: "text",
				text: successHint,
				metadata: { compactionSafe: true }
			} as any)
		}

		return recoveryResult
	}

	// --- Diagnostics & Classification ---

	private extractErrorCode(result: any): string | null {
		// Heuristic to extract error code from tool response
		if (result && Array.isArray(result) && result.length > 0) {
			const lastBlock = result[result.length - 1]
			if (lastBlock.type === "text" && lastBlock.text.includes("Error")) {
				const text = lastBlock.text
				if (text.includes("ENOENT")) return "FILE_NOT_FOUND"
				if (text.includes("ANCHOR_NOT_FOUND") || text.includes("anchor.notFound")) return "ANCHOR_NOT_FOUND"
				if (text.includes("EISDIR")) return "EISDIR"
				if (text.includes("ENOTDIR")) return "ENOTDIR"
				if (text.includes("Missing required parameter")) return "MISSING_PARAMETER"
				if (text.includes("Invalid argument") || text.includes("is not a valid")) return "INVALID_ARGUMENT"
				if (text.includes("EADDRINUSE")) return "EADDRINUSE"
				if (text.includes("maximum context") || text.includes("too many tokens") || text.includes("context length")) return "CONTEXT_OVERFLOW"
				if (text.includes("No files found") || text.includes("0 symbols found")) return "EMPTY_SEARCH_RESULTS"
				if (text.includes("blocked by the .diracignore file")) return "DIRAC_IGNORE_BLOCK"
				
				return "GENERIC_ERROR" // Fallback if it looks like an error but no code is found
			}
		}
		return null
	}

	private extractErrorMessage(errorOrResult: any): string {
		if (errorOrResult instanceof Error) {
			return errorOrResult.message
		}
		if (Array.isArray(errorOrResult)) {
			const texts = errorOrResult.filter((b: any) => b.type === "text").map((b: any) => b.text)
			return texts.join("\n")
		}
		return String(errorOrResult)
	}

	private classifyErrorDomain(errorCode: string, errorMessage: string): string {
		const lowerMsg = errorMessage.toLowerCase()
		if (lowerMsg.includes("anchor") || lowerMsg.includes("line")) return "ACTION"
		if (lowerMsg.includes("symbol") || lowerMsg.includes("not found") || lowerMsg.includes("enoent")) return "MEMORY"
		if (lowerMsg.includes("timeout") || lowerMsg.includes("rate limit") || lowerMsg.includes("lock") || lowerMsg.includes("access") || lowerMsg.includes("perm") || lowerMsg.includes("addr")) return "SYSTEM"
		if (lowerMsg.includes("context") || lowerMsg.includes("token")) return "SYSTEM"
		return "PLANNING"
	}

	private classifyFailureCategory(errorCode: string, errorMessage: string): string {
		const lowerMsg = errorMessage.toLowerCase()
		if (lowerMsg.includes("timeout") || lowerMsg.includes("rate limit") || lowerMsg.includes("lock") || lowerMsg.includes("econnreset") || lowerMsg.includes("addr")) {
			return "Transient"
		}
		if (lowerMsg.includes("context length") || lowerMsg.includes("too many tokens") || lowerMsg.includes("maximum context")) {
			return "Context Overflow"
		}
		if (lowerMsg.includes("not found") || lowerMsg.includes("does not exist") || lowerMsg.includes("invalid") || lowerMsg.includes("mismatch")) {
			return "Semantic Mismatch"
		}
		if (lowerMsg.includes("access") || lowerMsg.includes("perm") || lowerMsg.includes("denied")) {
			return "Permission Denied"
		}
		return "Unknown"
	}

	/**
	 * L-ICL Principle: Inject structured contextual messages instead of raw error blobs.
	 */
	private formatStructuredEscalation(toolName: string, args: unknown, errorCode: string, message: string, nextSteps?: string): ToolResponse {
		let argsStr = ""
		try {
			argsStr = JSON.stringify(args)
		} catch {
			argsStr = String(args)
		}

		const structuredMessage = `[SYSTEM: RECOVERY_FAILED]
BLOCKED: ${toolName} with arguments: ${argsStr}
REASON: ${errorCode} — ${message}
NEXT: ${nextSteps || "Please analyze the error and try a different approach or tool."}`

		return [
			{
				type: "text",
				text: structuredMessage,
			},
		] as any
	}

	// --- Stagnation Detection ---

	private fingerprintToolCall(toolName: string, args: unknown): string {
		// Create a semantic fingerprint.
		// E.g., for edit_file, we care about the target path, not necessarily the exact new_string if it differs slightly.
		// For simplicity, a shallow stringify of keys and core values, or a full hash if we don't have deep tool knowledge.
		let fingerprintStr = `${toolName}:`
		if (typeof args === "object" && args !== null) {
			const argObj = args as Record<string, any>
			if (toolName === DiracDefaultTool.EDIT_FILE && argObj.file_path) {
				fingerprintStr += `file_path=${argObj.file_path}`
			} else if (toolName === DiracDefaultTool.BASH && argObj.command) {
				fingerprintStr += `command=${argObj.command}`
			} else {
				// Fallback: shallow hash of keys
				fingerprintStr += Object.keys(argObj).sort().join(",")
			}
		} else {
			fingerprintStr += String(args)
		}
		return fingerprintStr
	}

	private recordCall(toolName: string, args: unknown) {
		this.callHistory.push({
			tool: toolName,
			fingerprint: this.fingerprintToolCall(toolName, args),
			timestamp: Date.now()
		})
		// Keep history bounded
		if (this.callHistory.length > 50) {
			this.callHistory.shift()
		}
	}

	private detectStagnation(toolName: string, args: unknown, taskState?: any): StagnationResult | null {
		const fingerprint = this.fingerprintToolCall(toolName, args)
		const currentFP = `${toolName}:${fingerprint}`
		
		// 1. Cycle Detection (L2 Backtracking)
		// We include the CURRENT call in the check to see if it completes a cycle
		const recent = this.callHistory.slice(-8) // Last 8 calls
		const fps = [...recent.map(c => `${c.tool}:${c.fingerprint}`), currentFP]
		const len = fps.length

		if (len >= 3) {
			// Pattern: A, A, A
			if (fps[len - 1] === fps[len - 2] && fps[len - 2] === fps[len - 3]) {
				return {
					stagnationDetected: true,
					summary: `Repeated identical semantic call to ${toolName} (3x). Loop broken.`
				}
			}
		}

		if (len >= 4) {
			// Pattern: A, B, A, B (Phase 13: Alternating Loop)
			if (fps[len - 1] === fps[len - 3] && fps[len - 2] === fps[len - 4]) {
				return {
					stagnationDetected: true,
					summary: `Alternating loop detected between ${toolName} and ${recent[recent.length - 1].tool}. Strategy thrashing.`
				}
			}
		}

		if (len >= 6) {
			// Pattern: A, B, C, A, B, C (Phase 14: Circular Loop)
			if (fps[len - 1] === fps[len - 4] && fps[len - 2] === fps[len - 5] && fps[len - 3] === fps[len - 6]) {
				return {
					stagnationDetected: true,
					summary: `Circular strategy loop detected (${toolName} -> ${recent[recent.length - 1].tool} -> ${recent[recent.length - 2].tool}).`
				}
			}
		}

		// 2. Non-progress File Loop Detection (Progress Awareness)
		if (taskState && taskState.filesTouchedInCurrentTurn.size >= 1) {
			const readOnlyTools = [DiracDefaultTool.FILE_READ, DiracDefaultTool.LIST_FILES, DiracDefaultTool.SEARCH, DiracDefaultTool.GET_FUNCTION, DiracDefaultTool.EXPAND_SYMBOL]
			const isReadOnly = readOnlyTools.includes(toolName as any)
			
			if (isReadOnly) {
				const recentReadOnlyCount = this.callHistory.slice(-5).filter(c => readOnlyTools.includes(c.tool as any)).length
				
				// Phase 5: Phase Tracking Nudge
				// We check if history + current call would hit the threshold
				if (recentReadOnlyCount + 1 >= 5 && taskState.currentTaskPhase === "exploration") {
					const touchedFiles = Array.from(taskState.filesTouchedInCurrentTurn)
					
					// Phase 11: Specification Drift Detection (Turn Threshold)
					if (taskState.totalToolCallCount > 30) {
						return {
							stagnationDetected: true,
							summary: `Extended exploration phase detected (${taskState.totalToolCallCount} total calls). Potential specification drift.`
						}
					}

					return {
						stagnationDetected: true,
						summary: `Extended exploration phase detected (${recentReadOnlyCount + 1} calls, ${touchedFiles.length} files). If you have enough context, please proceed to editing.`
					}
				}

				// Phase 5: Token Efficiency Nudge
				if (taskState.turnTokenEstimates > 10000 && isReadOnly) {
					return {
						stagnationDetected: true,
						summary: `High token consumption detected for exploration. Consider using targeted tools like expand_symbol or search_symbols.`
					}
				}
			}
		}

		return null
	}

	// --- Circuit Breaker ---

	private checkCircuit(toolName: string): CircuitState {
		let circuit = this.circuitBreakers.get(toolName)
		if (!circuit) {
			circuit = { state: "CLOSED", failures: 0, lastFailureTime: 0 }
			this.circuitBreakers.set(toolName, circuit)
		}

		if (circuit.state === "OPEN") {
			// Check if cooldown period (e.g., 30s) has passed to transition to HALF_OPEN
			if (Date.now() - circuit.lastFailureTime > 30000) {
				circuit.state = "HALF_OPEN"
				this.circuitBreakers.set(toolName, circuit)
			}
		}
		return circuit
	}

	private updateCircuit(toolName: string, success: boolean) {
		const circuit = this.circuitBreakers.get(toolName)
		if (!circuit) return

		if (success) {
			circuit.failures = 0
			circuit.state = "CLOSED"
		} else {
			circuit.failures++
			circuit.lastFailureTime = Date.now()
			// Open circuit after 3 consecutive failures
			if (circuit.failures >= 3) {
				circuit.state = "OPEN"
			}
		}
		this.circuitBreakers.set(toolName, circuit)
	}

	// --- Failure Memory (Graduated Recovery) ---

	private recordRecovery(errorCode: string, success: boolean) {
		const updateMap = (targetMap: Map<string, FailureRecord>, isGlobal: boolean) => {
			let record = targetMap.get(errorCode)
			if (!record) {
				record = { errorCode, tool: "", successCount: 0, failureCount: 0, lastSeen: 0 }
			}

			record.lastSeen = Date.now()
			if (success) {
				record.successCount++
			} else {
				record.failureCount++
				// Phase 7: Correction Learning (Memelord pattern)
				// If a specific recovery pattern fails 3+ times in a session, mark it as permanentSkip
				// Note: Skips are always local (isGlobal=false) to avoid project-specific failure pollution
				if (!isGlobal && record.failureCount >= 3 && record.successCount < 3) {
					record.permanentSkip = true
				}
			}
			targetMap.set(errorCode, record)
		}

		updateMap(this.failureMemory, false)
		
		// Only synchronize successes to the Global Playbook. 
		// Failures and skips stay local to prevent project-specific pollution.
		if (success) {
			updateMap(this.globalFailureMemory, true)
		}
		
		this.saveRecoveryMemory()
	}

	private shouldSkipRecovery(errorCode: string): boolean {
		if (this.sessionSkips.has(errorCode)) {
			return true
		}

		const localRecord = this.failureMemory.get(errorCode)
		const globalRecord = this.globalFailureMemory.get(errorCode)
		
		if (localRecord?.permanentSkip || globalRecord?.permanentSkip) {
			return true
		}

		// A pattern is graduated if it has 3+ successful recoveries across all projects
		const totalSuccess = (localRecord?.successCount || 0) + (globalRecord?.successCount || 0)
		const totalFailure = (localRecord?.failureCount || 0) + (globalRecord?.failureCount || 0)

		// If a pattern has failed 3+ times without graduating, skip it
		if (totalFailure >= 3 && totalSuccess < 3) {
			return true
		}
		return false
	}
}
