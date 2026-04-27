export type ConstraintID =
	| "noSelfReview"
	| "noExplanations"
	| "noCommentary"
	| "strictJSON"
	| "noApologies"
	| "bareMinimum"
	| string

export interface DAGSummary {
	nodes: number
	edges: number
	blastRadius?: Record<string, number>
	criticalPaths?: string[]
}

export interface ErrorContext {
	failedOps: Array<{ anchor: string; reason: string }>
	diagnostics: string[]
	diffSnippet?: string
}

export interface PromptConfig {
	baseSystem: string
	task: string
	schemaVersion?: string
	dagSummary?: DAGSummary
	errorContext?: ErrorContext
	constraints?: ConstraintID[]
	phase: number
	mode?: "interactive" | "predictive"
}
