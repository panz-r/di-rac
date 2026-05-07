export type ObservationType = "summary" | "watcher" | "filter" | "critic" | "reflection"

export type CriticAction = "CONTINUE" | "REFLECT" | "RESTART"

export interface ObserverConfig {
	enabled: boolean
	provider?: string
	modelId?: string
	observerTurns: number // Base frequency for Watcher (S1)
	criticFrequency: number // Base frequency for Critic (S2)
	tokenThreshold: number
	bufferActivation: number
	blockAfter: number | false
	reflectionEnabled: boolean
	reflectionTokenThreshold: number
    confidenceThreshold: number // Min confidence to inject into context
}

export interface ObservationEntry {
	timestamp: number
	type: ObservationType
	observationText: string
	compressedRange?: [number, number]
	tokenEstimate: number
	confidence?: number
	criticAction?: CriticAction
}

/**
 * SUMMARIZER: Focuses on "What happened" to compress history.
 */
export const OBSERVER_SUMMARIZER_PROMPT = `You are a Context Summarizer. Compress conversation messages into timestamped observations.

RULES:
1. Preserve EXACT details: file paths, line numbers, error messages, function names, decisions.
2. Discard: greetings, verbose tool outputs, reasoning steps, clarifications, filler.
3. Priority emojis: 📕=critical(decisions,errors), 📗=important(progress,tests), 📙=context(discussion)
4. Format: 📕 YYYY-MM-DD HH:MM — what happened with exact details
5. Output ONLY observation text. No preamble, no JSON.
`

/**
 * WATCHER (Observer S1): Fast pattern matcher, gap detector.
 */
export const OBSERVER_WATCHER_PROMPT = `You are a Watcher Critic (System 1). Analyze the recent trajectory and identify immediate gaps or patterns.

RULES:
1. Identify GAP: "Haven't checked file X", "Missing import Y", "Ignored instruction Z".
2. Identify PATTERN: "Repeated regex attempt", "Looping on error A".
3. Provide a confidence score (0.0 to 1.0).
4. Format: [OBSERVER:WATCHER | confidence:0.XX] [Insight] [END_OBSERVER]
5. Be EXTREMELY brief. Max 2 alerts.
6. Output ONLY the alerts or "No alerts."
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
 * CRITIC (Observer S2): Slow evaluator, decides to intervene.
 */
export const OBSERVER_CRITIC_PROMPT = `You are an Observer Critic (System 2). Evaluate the agent's progress toward the goal.

SECTIONS TO ANALYZE:
- TRAJECTORY: Is the agent moving closer to the goal or spinning wheels?
- EFFICIENCY: Is context being used effectively?
- DECISION: Should the agent CONTINUE, REFLECT (summarize and pivot), or RESTART (start fresh)?

FORMAT:
[OBSERVER:CRITIC | action:ACTION | confidence:0.XX]
REASON: [Reason for the decision]
SUGGESTION: [Specific course correction if not CONTINUE]
[END_OBSERVER]

ACTION values: CONTINUE, REFLECT, RESTART
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

export function buildObserverConfig(settings: {
	observerEnabled: boolean
	observerProvider?: string
	observerModelId?: string
	observerTurns: number
	observerCriticFrequency?: number
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
	}
}
