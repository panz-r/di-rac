export type ObservationType = "summary" | "watcher" | "filter" | "reflection"

export interface ObserverConfig {
	enabled: boolean
	provider?: string
	modelId?: string
	observerTurns: number
	tokenThreshold: number
	bufferActivation: number
	blockAfter: number | false
	reflectionEnabled: boolean
	reflectionTokenThreshold: number
}

export interface ObservationEntry {
	timestamp: number
	type: ObservationType
	observationText: string
	compressedRange?: [number, number]
	tokenEstimate: number
}

/**
 * SUMMARIZER: Focuses on "What happened" to compress history. (Context Compression)
 */
export const OBSERVER_SUMMARIZER_PROMPT = `You are a Context Summarizer. Compress conversation messages into timestamped observations.

RULES:
1. Preserve EXACT details: file paths, line numbers, error messages, function names, decisions.
2. Discard: greetings, verbose tool outputs, reasoning steps, clarifications, filler.
3. Priority emojis: 📕=critical(decisions,errors), 📗=important(progress,tests), 橙=context(discussion)
4. Format: 📕 YYYY-MM-DD HH:MM — what happened with exact details
5. Output ONLY observation text. No preamble, no JSON.
`

/**
 * WATCHER: Focuses on "What's missing/wrong". (Gap & Dead-end Detection)
 */
export const OBSERVER_WATCHER_PROMPT = `You are a Watcher Critic. Analyze the trajectory of the coding agent and identify gaps or dead-ends.

RULES:
1. Identify GAP: "Agent hasn't checked file X yet", "Missing import for Y", "Ignored user instruction Z".
2. Identify DEAD-END: "You've tried this regex 3 times without success", "Looping on same error in file A".
3. Identify OPPORTUNITY: "You could use tool B to solve this faster".
4. Format: [OBSERVER:WATCHER | confidence:0.XX] [Insight] [END_OBSERVER]
5. Be EXTREMELY brief and direct. Max 3 alerts.
6. Provide a confidence score (0.0 to 1.0) based on how certain you are of the gap or dead-end.
7. Output ONLY the alerts or "No alerts." if everything is optimal.
`

/**
 * FILTER: Focuses on "What is irrelevant". (Relevance Filtering)
 */
export const OBSERVER_FILTER_PROMPT = `You are a Relevance Filter. Identify irrelevant information currently in the agent's context.

RULES:
1. List files or messages that are no longer relevant to the CURRENT task.
2. Suggest what to "prune" to save tokens and reduce noise.
3. Format: [OBSERVER:FILTER | confidence:0.XX] [Suggestion] [END_OBSERVER]
4. Provide a confidence score (0.0 to 1.0).
5. Output ONLY suggestions or "Context clean." if all info is relevant.
`

export const REFLECTOR_SYSTEM_PROMPT = `You are a Reflector agent. Restructure and condense an observation log into a working context document.

SECTIONS:
- CURRENT STATE: What is the agent actively working on?
- KEY DECISIONS: What was decided and why?
- TECHNICAL CHANGES: Files modified, functions added/removed, signatures changed.
- WATCHER INSIGHTS: Consolidated gaps/dead-ends identified by the watcher.
- OUTSTANDING: TODOs, unresolved errors.

RULES:
1. Combine related observations.
2. Preserve all 📕 (critical) details.
3. Consolidate summaries.
4. Output ONLY the structured context document. No preamble.
`

export function buildObserverConfig(settings: {
	observerEnabled: boolean
	observerProvider?: string
	observerModelId?: string
	observerTurns: number
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
		tokenThreshold: settings.observerTokenThreshold,
		bufferActivation: settings.observerBufferActivation,
		blockAfter: settings.observerBlockAfter,
		reflectionEnabled: settings.observerReflectionEnabled,
		reflectionTokenThreshold: settings.observerReflectionTokenThreshold,
	}
}
