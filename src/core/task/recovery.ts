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

	constructor() {
		this.initializeRecoveryTable()
		this.loadRecoveryMemory()
	}

	private async loadRecoveryMemory() {
		try {
			const fs = await import("fs/promises")
			const path = await import("path")
			const memoryFile = path.join(process.cwd(), ".dirac-state", "recovery-memory.json")
			const content = await fs.readFile(memoryFile, "utf-8")
			const data = JSON.parse(content)
			
			const THIRTY_DAYS_MS = 30 * 24 * 60 * 60 * 1000
			Object.entries(data).forEach(([key, value]) => {
				const record = value as FailureRecord
				// Weight decay: if not seen in 30 days, demote (reduce success count)
				if (Date.now() - record.lastSeen > THIRTY_DAYS_MS && record.successCount >= 3) {
					record.successCount = 2 // Demote from graduated
				}
				this.failureMemory.set(key, record)
			})
		} catch (e) {
			// Ignore if file doesn't exist or is malformed
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
		try {
			const fs = await import("fs/promises")
			const path = await import("path")
			const diracStateDir = path.join(process.cwd(), ".dirac-state")
			await fs.mkdir(diracStateDir, { recursive: true })
			const memoryFile = path.join(diracStateDir, "recovery-memory.json")
			const data = Object.fromEntries(this.failureMemory.entries())
			await fs.writeFile(memoryFile, JSON.stringify(data, null, 2))
		} catch (e) {
			// Ignore errors
		}
	}

	public resetTurnBudget() {
		this.currentTurnRetries = 0
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
	public async runPreflightFirewall(block: ToolUse, taskState: any): Promise<ToolResponse | null> {
		const toolName = block.name
		const params = block.params as any

		// --- Stage I: Argument Extraction ---
		const filePath = params?.path || params?.file_path || params?.absolutePath
		const startLine = params?.start_line
		const endLine = params?.end_line

		// --- Stage II: Content & State Scanning ---
		
		// 0. Phase and Token Tracking (Phase 5)
		if (taskState) {
			const mutationTools = [DiracDefaultTool.EDIT_FILE, DiracDefaultTool.FILE_NEW, DiracDefaultTool.BASH]
			const verificationTools = [DiracDefaultTool.BASH_RESTRICTED, DiracDefaultTool.DIAGNOSTICS_SCAN]
			
			if (mutationTools.includes(toolName as any) && taskState.currentTaskPhase === "exploration") {
				taskState.currentTaskPhase = "editing"
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

		// 1. Paradoxical Ranges Check
		if (toolName === DiracDefaultTool.FILE_READ && typeof startLine === "number" && typeof endLine === "number") {
			if (startLine > endLine) {
				// Stage III Policy: Silent Fix
				block.params.start_line = endLine
				block.params.end_line = startLine
				this.updateAuditChain(toolName, "PARADOXICAL_RANGE", "SILENT_FIX")
			}
		}

		// 2. Stale Context Check
		if (toolName === DiracDefaultTool.EDIT_FILE && typeof filePath === "string") {
			const lastAccess = taskState.fileLastAccessToolIndex.get(filePath)
			const currentCount = taskState.totalToolCallCount
			
			if (lastAccess !== undefined && (currentCount - lastAccess) > 15) {
				// Stage III Policy: Stale Context Block
				this.updateAuditChain(toolName, "STALE_CONTEXT", "BLOCKED")
				return this.formatStructuredEscalation(
					toolName,
					block.params,
					"STALE_CONTEXT",
					`The context for '${filePath}' is stale (last read ${currentCount - lastAccess} tool calls ago).`,
					`Please use read_file (detail="outline" or normal) to refresh your context before attempting an edit.`
				)
			}
		}

		// 3. Overlapping Edit Check
		if (toolName === DiracDefaultTool.EDIT_FILE && typeof filePath === "string") {
			if (taskState.filesTouchedInCurrentTurn.has(filePath)) {
				// Stage III Policy: Overlapping Edit Warning/Block
				// For now, let's just log it and allow it if it's not graduated to a full block
				this.updateAuditChain(toolName, "OVERLAPPING_EDIT", "WARNING")
			}
		}

		// Update tracking state (AEGIS Stage I/II side effect)
		if (filePath && typeof filePath === "string") {
			taskState.fileLastAccessToolIndex.set(filePath, taskState.totalToolCallCount)
			taskState.filesTouchedInCurrentTurn.add(filePath)
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

					// Extract the new line number for the anchor from the outline
					// (Implementation would need to match anchor text/id to the new outline)
					// For now, return the outline to the LLM so it can pick the correct anchor
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

					// Automatically re-read outline to show what changed
					const outlineResult = await execute(DiracDefaultTool.FILE_READ, {
						path: filePath,
						detail: "outline"
					})

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

							// Retry the tool with the relative path
							return await execute(toolName as any, newInput)
						}
					} catch (e) {
						// Fall through to escalation
					}

					return null // Escalate to LLM
				},
			},
			FILE_NOT_FOUND: {
				domain: ErrorDomain.MEMORY,
				category: ErrorCategory.PERMANENT,
				tier: "input_error",
				maxRetries: 0,
				handler: async (_toolName, _input, _error, _attempt, _execute) => {
					return null // pass through -- LLM needs to pick different path
				},
			},
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
			this.updateAuditChain(toolName, "STAGNATION_DETECTED", "BLOCKED")
			const isExactRepeat = stagnation.summary.includes("identical")
			return this.formatStructuredEscalation(
				toolName,
				args,
				"STAGNATION_DETECTED",
				stagnation.summary,
				isExactRepeat 
					? "You are repeating the same action. Please reconsider your approach or check the tool parameters."
					: "You have been exploring without making progress. Consider switching to editing or refine your search."
			)
		}

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
			recoveryResult.push({
				type: "text",
				text: "[SYSTEM: DETERMINISTIC_RECOVERY_SUCCESS - COMPACTION_SAFE]",
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
				// Naive extraction for now - needs refinement based on actual ToolResponse format
				if (lastBlock.text.includes("ENOENT")) return "FILE_NOT_FOUND"
				if (lastBlock.text.includes("ANCHOR_NOT_FOUND")) return "ANCHOR_NOT_FOUND"
				// ...
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
		if (lowerMsg.includes("timeout") || lowerMsg.includes("rate limit") || lowerMsg.includes("lock")) return "SYSTEM"
		return "PLANNING"
	}

	private classifyFailureCategory(errorCode: string, errorMessage: string): string {
		const lowerMsg = errorMessage.toLowerCase()
		if (lowerMsg.includes("timeout") || lowerMsg.includes("rate limit") || lowerMsg.includes("lock") || lowerMsg.includes("econnreset")) {
			return "Transient"
		}
		if (lowerMsg.includes("context length") || lowerMsg.includes("too many tokens") || lowerMsg.includes("maximum context")) {
			return "Context Overflow"
		}
		if (lowerMsg.includes("not found") || lowerMsg.includes("does not exist") || lowerMsg.includes("invalid") || lowerMsg.includes("mismatch")) {
			return "Semantic Mismatch"
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
		
		// 1. Exact/Semantic Repeat (L2 Backtracking)
		const recentCalls = this.callHistory.slice(-3)
		if (recentCalls.length === 3) {
			const allMatch = recentCalls.every(c => c.tool === toolName && c.fingerprint === fingerprint)
			if (allMatch) {
				return {
					stagnationDetected: true,
					summary: `Repeated identical semantic call to ${toolName} (3x). Loop broken to prevent token exhaustion.`
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
				if (recentReadOnlyCount >= 5 && taskState.currentTaskPhase === "exploration") {
					const touchedFiles = Array.from(taskState.filesTouchedInCurrentTurn)
					return {
						stagnationDetected: true,
						summary: `Extended exploration phase detected (${recentReadOnlyCount} calls, ${touchedFiles.length} files). If you have enough context, please proceed to editing.`
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
		let record = this.failureMemory.get(errorCode)
		if (!record) {
			record = { errorCode, tool: "", successCount: 0, failureCount: 0, lastSeen: 0 }
		}

		record.lastSeen = Date.now()
		if (success) {
			record.successCount++
		} else {
			record.failureCount++
		}

		this.failureMemory.set(errorCode, record)
		this.saveRecoveryMemory()
	}

	private shouldSkipRecovery(errorCode: string): boolean {
		if (this.sessionSkips.has(errorCode)) {
			return true
		}

		const record = this.failureMemory.get(errorCode)
		if (!record) return false

		// If a pattern has failed 3+ times without graduating, skip it
		if (record.failureCount >= 3 && record.successCount < 3) {
			return true
		}
		return false
	}
}
