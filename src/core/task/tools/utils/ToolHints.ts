import type { DiracDefaultTool } from "@/shared/tools"
import type { ToolError } from "@/shared/tool-response"

/**
 * Unified hint registry for tool success and error contexts.
 * Consolidates guidance from:
 *   - buildExplorationHint() in ToolExecutorCoordinator (success hints)
 *   - formatToolErrorGuidance() in responses.ts (error hints)
 */

type HintContext = "success" | "error"

export function getToolHint(
	toolName: string,
	context: HintContext,
	errorCode?: string,
	params?: Record<string, any>,
): string | undefined {
	if (context === "success") {
		return getSuccessHint(toolName, params)
	}
	if (errorCode) {
		return getErrorHint(errorCode, params)
	}
	return undefined
}

// ── Success Hints ────────────────────────────────────────────────────────────

function getSuccessHint(tool: string, params?: Record<string, any>): string | undefined {
	const path = params?.path ?? (Array.isArray(params?.paths) ? params.paths[0] : undefined)

	switch (tool) {
		case "read":
			return path
				? `Follow-up: symbols ${path} --action expand --symbol <handle> | read ${path} --detail outline`
				: undefined
		case "search":
			return "Follow-up: read <matched-path> | repo <matched-path>"
		case "repo":
			return "Follow-up: read <path> | symbols search --name <query> | search <path> --pattern <regex>"
		case "symbols":
			return "Follow-up: symbols <path> --action expand --symbol <handle> | symbols <path> refs --name <name>"
		default:
			return undefined
	}
}

// ── Error Hints ──────────────────────────────────────────────────────────────

function getErrorHint(code: string, params?: Record<string, any>): string | undefined {
	switch (code) {
		case "io.file.notFound": {
			const dir = params?.dir ?? "<parent-dir>"
			return `File not found. Try: repo ${dir} to list files, or search --pattern <name> to find it.`
		}
		case "io.file.permissionDenied":
			return "Permission denied. Check file permissions or use a different path."
		case "io.file.locked":
			return "File locked by another process. Wait and retry."
		case "anchor.notFound":
			return "Anchor not found. Re-read the file (read --detail outline) to get fresh anchors."
		case "anchor.contentMismatch":
			return "Content at anchor has changed. Re-read the file before editing."
		case "anchor.invalidFormat":
			return "Invalid anchor format. Use hash-anchored lines from read --detail outline."
		case "edit.multiFileConflict":
			return "Multi-file conflict. Edit each file separately."
		case "lsp.timeout":
			return "Language server timed out. Retry or use a non-AST approach."
		case "lsp.connectionLost":
			return "Language server connection lost. Retry — it may recover."
		case "arg.invalidArgument":
			return "Invalid argument. Check parameter types and retry."
		case "tool.unknownError":
			return "Unexpected error. Re-read relevant files to ensure context is current."
		case "tool.internalError":
			return "Internal error. Retry once, or try an alternative approach."
		default:
			return undefined
	}
}

/**
 * Build the full error guidance string from a ToolError (replaces formatToolErrorGuidance).
 */
export function formatErrorGuidance(error: ToolError): string {
	const hint = getErrorHint(error.code, error.details as Record<string, any> | undefined)
	if (hint) return hint

	// Fallback for codes not in the registry
	return `Tool execution failed${error.message ? ": " + error.message : ""}. Try a different approach or check your inputs.`
}
