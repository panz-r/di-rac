import { PromptConfig, ConstraintID } from "./types"
import { CONSTRAINT_TEXT } from "./constraints"
import { MANIFEST_SCHEMAS } from "./schemas"

export function buildPrompt(config: PromptConfig): string {
	const lines: string[] = []

	// 1. System base
	lines.push(config.baseSystem)

	// 2. Mode indicator
	if (config.mode === "predictive") {
		lines.push("MODE: You are in predictive execution mode. Follow the output format exactly.")
	}

	// 3. Task (skip if empty — allows reuse for system-prompt-only builds)
	if (config.task) {
		lines.push(`\n## Task\n${config.task}`)
	}

	// 4. DAG summary (very compact)
	if (config.dagSummary) {
		const d = config.dagSummary
		lines.push(`\n## Dependencies\n- ${d.nodes} files, ${d.edges} relationships`)
		if (d.blastRadius) {
			const high = Object.entries(d.blastRadius)
				.filter(([, v]) => v > 5)
				.map(([k]) => k)
			if (high.length) lines.push(`- High‑impact: ${high.join(", ")}`)
		}
		if (d.criticalPaths?.length) {
			lines.push(`- Critical path: ${d.criticalPaths.join(" → ")}`)
		}
	}

	// 5. Schema
	if (config.schemaVersion) {
		const desc = MANIFEST_SCHEMAS[config.schemaVersion]
		if (desc) lines.push(`\n## Output Format\n${desc}`)
	}

	// 6. Error context (corrective turn)
	if (config.errorContext) {
		const err = config.errorContext
		lines.push(`\n## Errors to Fix`)
		for (const op of err.failedOps) {
			lines.push(`- Anchor "${op.anchor}": ${op.reason}`)
		}
		if (err.diagnostics.length) {
			lines.push(`\nLinter/Type Errors:\n${err.diagnostics.join("\n")}`)
		}
		if (err.diffSnippet) {
			lines.push(`\nDiff of your previous edit:\n\`\`\`diff\n${err.diffSnippet}\n\`\`\``)
		}
	}

	// 7. Constraints (only append if explicitly requested to avoid duplication with base system)
	if (config.constraints && config.constraints.length > 0) {
		const unique = [...new Set(config.constraints)]
		lines.push("\n---\n## Rules")
		for (const id of unique) {
			const text = CONSTRAINT_TEXT[id]
			if (text) lines.push(`- ${text}`)
		}
	}

	return lines.join("\n\n")
}

export function validatePromptConfig(config: PromptConfig): void {
	if (!config.baseSystem) throw new Error("baseSystem is required")
	if (config.phase < 1 || config.phase > 5) throw new Error("phase must be 1-5")
	// Predictive mode requires schema version
	if (config.mode === "predictive" && config.phase >= 2 && !config.schemaVersion) {
		throw new Error("Predictive phase >=2 requires schemaVersion")
	}
}
