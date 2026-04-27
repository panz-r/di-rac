/**
 * Dirac Tool Response — Lean discriminated-union error handling for tool calls.
 *
 * Handlers return `ToolResponse<T>` directly. The agent loop routes errors
 * via `routeToolError()` and serializes them for the LLM at the boundary via
 * `formatResponse.formatToolErrorForLLM()`.
 */

// ── Severity ────────────────────────────────────────────────────────────────

export type ErrorSeverity = "recoverable" | "unrecoverable" | "fatal"

// ── Error Codes ─────────────────────────────────────────────────────────────

export type ToolErrorCode =
	// I/O
	| "io.file.notFound"
	| "io.file.permissionDenied"
	| "io.file.locked"
	// Argument validation
	| "arg.invalidArgument"
	// Anchor & Edit
	| "anchor.notFound"
	| "anchor.contentMismatch"
	| "anchor.invalidFormat"
	| "edit.multiFileConflict"
	// LSP / diagnostics
	| "lsp.timeout"
	| "lsp.connectionLost"
	// Manifest (future)
	| "manifest.invalidSchema"
	| "manifest.duplicateOp"
	| "manifest.orderingConflict"
	// Speculative runner (future)
	| "speculative.workspace.rejected"
	| "speculative.verify.failed"
	// Generic / legacy
	| "tool.unknownError"
	| "tool.internalError"

// ── ToolError ───────────────────────────────────────────────────────────────

export type ToolError = {
	/** Machine-readable, dot-namespaced code (e.g. "io.file.notFound") */
	code: ToolErrorCode
	/** Stable message for logging/diagnostics — NOT shown to the LLM directly */
	message: string
	/** Routing severity */
	severity: ErrorSeverity
	/** Optional dynamic context (e.g. { path, anchor, line }) */
	details?: Record<string, unknown>
	/** Optional loosely-typed metadata */
	metadata?: {
		timestamp?: number
		phase?: number
		retryCount?: number
		[key: string]: unknown
	}
}

// ── ToolResponse ────────────────────────────────────────────────────────────

export type ToolResponse<T = unknown> =
	| { success: true; data: T; warnings?: ToolError[] }
	| { success: false; error: ToolError; warnings?: ToolError[] }

// ── Error Action (for agent loop routing) ──────────────────────────────────

export type ErrorAction =
	| { type: "retry"; maxTimes: number }
	| { type: "abort" }
	| { type: "ask-user" }
	| { type: "ignore" }

// ── Factory ─────────────────────────────────────────────────────────────────

/**
 * Create a structured ToolError with sensible defaults.
 *
 * Usage in handlers:
 *   createToolError("anchor.notFound", `Anchor '${anchor}' not found`, "recoverable", { anchor, file })
 */
export function createToolError(
	code: ToolErrorCode,
	message: string,
	severity: ErrorSeverity = "recoverable",
	details?: Record<string, unknown>,
	metadata?: ToolError["metadata"],
): ToolError {
	return {
		code,
		message,
		severity,
		...(details ? { details } : {}),
		metadata: {
			timestamp: Date.now(),
			...(metadata || {}),
		},
	}
}

// ── Router ──────────────────────────────────────────────────────────────────

/**
 * Route a tool error to an action based on error code, phase, and tool.
 *
 * The `phase` parameter comes from the session/context:
 *   - 2 = interactive act mode (normal task execution)
 *   - 3 = interactive with checkpointing
 *   - 4 = speculative pre-computation
 *   - 5 = predictive verification
 *
 * For now Dirac's mode is binary ("plan" | "act"), so phase defaults to 2 for
 * "act" mode and can be refined later.
 */
export function routeToolError(
	error: ToolError,
	toolName: string,
	phase: number,
): ErrorAction {
	switch (error.code) {
		case "io.file.notFound":
		case "io.file.permissionDenied":
			return phase <= 3 ? { type: "retry", maxTimes: 2 } : { type: "abort" }

		case "anchor.notFound":
		case "anchor.contentMismatch":
		case "anchor.invalidFormat":
			return phase <= 4 ? { type: "retry", maxTimes: 1 } : { type: "abort" }

		case "edit.multiFileConflict":
			return { type: "abort" }

		case "lsp.timeout":
		case "lsp.connectionLost":
			return { type: "retry", maxTimes: 1 }

		case "manifest.invalidSchema":
		case "manifest.duplicateOp":
		case "manifest.orderingConflict":
			return { type: "abort" }

		case "speculative.workspace.rejected":
		case "speculative.verify.failed":
			return phase >= 4 ? { type: "ignore" } : { type: "abort" }

		case "arg.invalidArgument":
			// Invalid arguments — retry once with corrected params
			return { type: "retry", maxTimes: 1 }

		case "tool.internalError":
			return { type: "abort" }

		// Default: recoverable → retry once, unrecoverable/fatal → abort
		default:
			return error.severity === "fatal" || error.severity === "unrecoverable"
				? { type: "abort" }
				: { type: "retry", maxTimes: 1 }
	}
}
