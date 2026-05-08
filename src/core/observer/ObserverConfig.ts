export type ObservationType = "summary" | "watcher" | "filter" | "critic" | "reflection" | "skeleton"

export type CriticAction = "CONTINUE" | "REFLECT" | "RESTART"

export type SkeletonFidelity = "full" | "structural" | "decision"

export type LoopPattern = "PRODUCTIVE" | "STUCK" | "WIDENING" | "OSCILLATING" | "STUCK_WITH_FORGETTING" | "UNKNOWN"

export interface LanguageNorm {
	medianEditChurn: number
	medianFileSize: number
	sci: number
}

export const LANGUAGE_NORMALIZATION: Record<string, LanguageNorm> = {
	python: { medianEditChurn: 15, medianFileSize: 200, sci: 12.0 },
	javascript: { medianEditChurn: 12, medianFileSize: 150, sci: 14.0 },
	typescript: { medianEditChurn: 12, medianFileSize: 150, sci: 14.0 },
	java: { medianEditChurn: 8, medianFileSize: 300, sci: 65.0 },
	go: { medianEditChurn: 10, medianFileSize: 200, sci: 31.0 },
	rust: { medianEditChurn: 6, medianFileSize: 250, sci: 80.0 },
	c: { medianEditChurn: 5, medianFileSize: 180, sci: 95.0 },
	cpp: { medianEditChurn: 5, medianFileSize: 180, sci: 122.0 },
	ruby: { medianEditChurn: 18, medianFileSize: 120, sci: 10.0 },
}

export interface ObserverConfig {
	enabled: boolean
	provider?: string
	modelId?: string
	observerTurns: number // S1 Watcher frequency
	criticFrequency: number // S2 Critic frequency
	tokenThreshold: number
	bufferActivation: number
	blockAfter: number | false
	reflectionEnabled: boolean
	reflectionTokenThreshold: number
	confidenceThreshold: number
	reflectionCooldown: number // Min turns between reflections (Singh et al. 2025)
	verbose: boolean
	tauWatcher: number // Time constant for S1 decay (Shen et al. 2025)
	tauCritic: number // Time constant for S2 decay
	permissiveBufferSize: number // Wong et al. 2025

	// Phase 5/7: Refined Thresholds
	tierThresholds: {
		sqs: [number, number, number, number] // SQS triggers for Tier 0, 1, 2, 3
		confidence: [number, number, number, number] // Min confidence for Tier 0, 1, 2, 3
	}
    
    // Phase 7: AST & Procedural Refinements
    proceduralMonotonicityEnabled: boolean
    astGuidedMemoryEnabled: boolean
    adaptiveCooldownEnabled: boolean
    
    // Weights for SQS
    sqsWeights: {
        diffusion: number
        eeRatio: number
        dcr: number
        cps: number
        monotonicity: number
    }

	// Phase 8: Resource Guarding
	latencyBudgetMs: number
	memoryCapTokens: number
}

export interface ObservationEntry {
	timestamp: number
	type: ObservationType
	observationText: string
	compressedRange?: [number, number]
	tokenEstimate: number
	confidence?: number
	criticAction?: CriticAction
	sqs?: number // Search Quality Score (Zheng et al. 2026)
	fidelity?: SkeletonFidelity
    
    // Key-Value Skeleton (CodeMEM 2026)
    key?: {
        signature: string
        apisCalled: string[]
        apisDefined: string[]
        docstringHash: string
    }
}

export interface ObserverMetrics {
	sqsComponents: {
		diffusion: number
		eeRatio: number
		dcr: number
		cps: number
		monotonicity: number
	}
	interventionTrigger: {
		structuralOnly: boolean
		userOnly: boolean
		combined: boolean
		confidenceCalibrated: number
	}
	forgettingEvents: {
		detected: number
		falsePositive: number
		resolvedByIntervention: number
		ifrScore: number
	}
	tokenEfficiency: {
		layer1CompressionRatio: number
		s2ValueLoads: number
		retrievalStageUsed: 1 | 2 | 3
	}
}

export function buildObserverConfig(settings: {
	observerEnabled: boolean
	observerProvider?: string
	observerModelId?: string
	observerTurns: number
	observerCriticFrequency?: number
	observerVerbose?: boolean
	observerTokenThreshold: number
	observerBufferActivation: number
	observerBlockAfter: number
	observerReflectionEnabled: boolean
	observerReflectionTokenThreshold: number
}): ObserverConfig {
	return {
		enabled: settings.observerEnabled,
		provider: settings.observerProvider,
		modelId: settings.observerModelId,
		observerTurns: settings.observerTurns,
		criticFrequency: settings.observerCriticFrequency || 6,
		tokenThreshold: settings.observerTokenThreshold,
		bufferActivation: settings.observerBufferActivation,
		blockAfter: settings.observerBlockAfter,
		reflectionEnabled: settings.observerReflectionEnabled,
		reflectionTokenThreshold: settings.observerReflectionTokenThreshold,
		confidenceThreshold: 0.5,
		reflectionCooldown: 4,
		verbose: settings.observerVerbose || false,
		tauWatcher: 7,
		tauCritic: 15,
		permissiveBufferSize: 2,
		tierThresholds: {
			sqs: [0.3, 0.32, 0.35, 0.4],
			confidence: [0.5, 0.55, 0.6, 0.7],
		},
        proceduralMonotonicityEnabled: true,
        astGuidedMemoryEnabled: true,
        adaptiveCooldownEnabled: true,
        sqsWeights: {
            diffusion: 0.30,
            eeRatio: 0.25,
            dcr: 0.20,
            cps: 0.15,
            monotonicity: 0.10
        },
		latencyBudgetMs: 200,
		memoryCapTokens: 15000,
	}
}

/**
 * SUMMARIZER: Context Compression (Pattern A)
 */
export const OBSERVER_SUMMARIZER_PROMPT = `You are a Context Summarizer. Compress conversation messages into timestamped observations.

RULES:
1. Preserve EXACT details: file paths, line numbers, error messages, function names.
2. Discard: filler, greetings, verbose tool reasoning.
3. Output ONLY observation text.
`

/**
 * WATCHER (Observer S1): Fast pattern matcher (Miao 2024 / Wong 2025)
 */
export const OBSERVER_WATCHER_PROMPT = `You are a Watcher Critic (System 1). Identify immediate gaps or loops.

RULES:
1. Identify GAP: "Missing check for X", "Ignored instruction Z".
2. Identify LOOP: "Repeated regex/edit attempt on same file".
3. Provide a confidence score (0.0 to 1.0).
4. Format: [OBSERVER:WATCHER | confidence:0.XX] [Insight] [END_OBSERVER]
`

/**
 * FILTER: Relevance Filtering.
 */
export const OBSERVER_FILTER_PROMPT = `You are a Relevance Filter. Identify irrelevant information currently in the agent's context.

RULES:
1. List items to "prune" (stagnant files, outdated logs).
2. Provide a confidence score (0.0 to 1.0).
3. Format: [OBSERVER:FILTER | confidence:0.XX] [Suggestion] [END_OBSERVER]
4. Output ONLY suggestions or "Context clean."
`

/**
 * CRITIC (Observer S2): Slow evaluator (Zheng 2026)
 */
export const OBSERVER_CRITIC_PROMPT = `You are an Observer Critic (System 2). Evaluate trajectory and SQS.

SECTIONS:
- TRAJECTORY: Is the agent focused or wandering (diffusion)?
- DECISION: Should the agent CONTINUE, REFLECT (summarize and pivot), or RESTART?

FORMAT:
[OBSERVER:CRITIC | action:ACTION | confidence:0.XX]
REASON: [Reason]
[END_OBSERVER]
`

/**
 * SKELETON: Structured Pruning (H2O 2025 / CodeMEM 2026)
 */
export const OBSERVER_SKELETON_PROMPT = `You are a Structured Pruner. Extract a lossless skeleton of the last 15-20 turns.

FORMAT:
- EDITS: {file: [signatures/types, ast_delta_nodes]}
- API_DEPS: {external_calls: [], internal_refs: []}
- ERRORS: [Brief error signatures]
- DECISIONS: [Key strategy shifts & rationales]
- TESTS: [Pass/Fail delta]

Output ONLY the JSON-like structure.
`

export const REFLECTOR_SYSTEM_PROMPT = `You are a Reflector agent. Restructure and condense an observation log into a working context document.

SECTIONS:
- CURRENT STATE: What is the agent actively working on?
- KEY DECISIONS: What was decided and why?
- TECHNICAL CHANGES: Files modified, functions changed.
- WATCHER INSIGHTS: Consolidated gaps/dead-ends.
- OUTSTANDING: TODOs, unresolved errors.

RULES:
1. Combine related observations.
2. Preserve all 📕 (critical) details.
3. Output ONLY the structured context document.
`
