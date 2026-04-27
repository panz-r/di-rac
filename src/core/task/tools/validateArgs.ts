/**
 * Pre-execution argument validation for tool calls.
 *
 * Validates raw LLM-supplied `block.params` against a Zod schema before
 * the handler ever sees them. Returns a structured ToolError on failure
 * so the LLM receives actionable, typed error messages instead of raw
 * type-mismatch crashes.
 *
 * Also includes normalisation helpers for common LLM artefacts like
 * ASCII-character-code integers appearing in string arrays.
 */
import type { ZodSchema } from "zod";
import { createToolError } from "@shared/tool-response";
import type { ToolError } from "@shared/tool-response";
import { Logger } from "@/shared/services/Logger";

// ---------------------------------------------------------------------------
// Validation result
// ---------------------------------------------------------------------------

export type ValidationResult<T> =
	| { success: true; data: T }
	| { success: false; error: ToolError };

// ---------------------------------------------------------------------------
// Main validation helper
// ---------------------------------------------------------------------------

/**
 * Validate raw tool arguments against a Zod schema.
 *
 * On success, returns the parsed (and coerced/transformed) data.
 * On failure, returns a structured `INVALID_ARGUMENT` ToolError with
 * human-readable issue messages and the raw input for diagnostics.
 */
export function validateArgs<T>(
	schema: ZodSchema<T>,
	rawArgs: unknown,
	toolName?: string,
): ValidationResult<T> {
	const result = schema.safeParse(rawArgs);

	if (result.success) {
		return { success: true, data: result.data };
	}

	// Build human-readable error messages from Zod issues
	const messages = result.error.issues.map((issue) => {
		const path = issue.path.length > 0 ? issue.path.join(".") : "<root>";
		return `${path}: ${issue.message}`;
	});

	const message = `Invalid arguments for ${toolName || "tool"}: ${messages.join("; ")}`;

	// Log the validation failure for diagnostics
	Logger.warn(`[validateArgs] ${message}`, {
		tool: toolName,
		received: JSON.stringify(rawArgs),
	});

	return {
		success: false,
		error: createToolError(
			"arg.invalidArgument",
			message,
			"recoverable",
			{
				received: rawArgs,
				issues: result.error.issues.map((i) => ({
					path: i.path,
					message: i.message,
					code: i.code,
				})),
			},
		),
	};
}

// ---------------------------------------------------------------------------
// Argument normalisation helpers
// ---------------------------------------------------------------------------

/**
 * Catch token-level artefacts like ASCII character code integers
 * appearing where strings are expected (e.g., 65 instead of "A").
 *
 * Returns the cleaned string array, or null if the input cannot be
 * safely normalised (in which case the Zod schema will reject it).
 */
export function safeStringArray(arr: unknown): string[] | null {
	if (!Array.isArray(arr)) {
		return null;
	}

	const result: string[] = [];
	for (const item of arr) {
		if (typeof item === "string") {
			result.push(item);
		} else if (typeof item === "number" && item > 0 && item < 256) {
			// Log and abort — do not guess the character
			Logger.warn(
				`[validateArgs] Dropping suspicious numeric path value in string array: ${item} (ASCII code)`,
			);
			return null;
		} else {
			return null;
		}
	}
	return result;
}
