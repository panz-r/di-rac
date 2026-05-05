import type { TaskState } from "../../TaskState"

/**
 * Compute a 0-1 ambiguity score for a tool invocation.
 * Higher = more ambiguous, should ask/retry rather than proceed blindly.
 *
 * Thresholds (Microsoft May 2025):
 *   > 0.6  →  retry recommended
 *   > 0.4  →  moderate, add guidance hint
 *   ≤ 0.4  →  proceed normally
 */
export function computeAmbiguityScore(
	tool: string,
	params: Record<string, any>,
	state: TaskState,
): number {
	let score = 0
	const p = params

	switch (tool) {
		case "read": {
			// No detail/range on a file read >3 times (likely lost)
			if (!p.start_line && !p.end_line && !p.range && !p.ranges && !p.detail) {
				const paths: string[] = p.paths ?? (p.path ? [p.path] : [])
				for (const fp of paths) {
					if ((state.readCounts.get(fp) || 0) > 3) score += 0.3
				}
			}
			break
		}
		case "search": {
			const pattern = p.regex || p.pattern || ""
			if (!pattern) score += 0.5
			else if (typeof pattern === "string" && pattern.length <= 2) score += 0.3
			break
		}
		case "edit": {
			const anchor = p.anchor || p.files?.[0]?.edits?.[0]?.anchor
			if (!anchor) score += 0.4
			// Editing a file never read in this task
			const editPath = p.path || p.files?.[0]?.path || ""
			if (editPath && !state.readCounts.has(editPath)) score += 0.2
			break
		}
		case "write": {
			if (!p.content && !p.text) score += 0.4
			break
		}
	}

	return Math.min(score, 1.0)
}
