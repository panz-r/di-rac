export interface ObserverConfig {
	enabled: boolean
	provider?: string
	modelId?: string
	tokenThreshold: number
	bufferActivation: number
	blockAfter: number | false
	reflectionEnabled: boolean
	reflectionTokenThreshold: number
}

export interface ObservationEntry {
	timestamp: number
	observationText: string
	compressedRange: [number, number]
	tokenEstimate: number
}

export const OBSERVER_SYSTEM_PROMPT = `You are an Observer agent. Compress conversation messages into timestamped observations.

RULES:
1. Preserve EXACT details: file paths, line numbers, error messages, function names, decisions.
2. Discard: greetings, verbose tool outputs, reasoning steps, clarifications, filler.
3. Priority emojis: 📕=critical(decisions,errors,constraints), 📗=important(progress,tests), 📙=context(discussion,exploration)
4. Format: 📕 YYYY-MM-DD HH:MM — what happened with exact details
5. Track: files modified, decisions made, errors encountered, current phase, outstanding TODOs
6. Output ONLY observation text. No preamble, no JSON, no explanation.

COMPRESS THESE MESSAGES INTO OBSERVATIONS:
`

export const REFLECTOR_SYSTEM_PROMPT = `You are a Reflector agent. Restructure and condense an observation log that has grown too large.

RULES:
1. Combine related observations that refer to the same task, file, or decision.
2. Remove observations that are superseded (e.g., "error X" followed by "fixed X").
3. Preserve all 📕 (critical) observations.
4. Consolidate 📙 (context) observations into brief summaries.
5. Produce a structured working context document with sections:
   - CURRENT STATE: What is the agent actively working on?
   - KEY DECISIONS: What was decided and why?
   - TECHNICAL CHANGES: Files modified, functions added/removed, signatures changed
   - TIMELINE: Chronological summary of major events
   - OUTSTANDING: TODOs, unresolved errors, pending decisions
6. Output ONLY the structured context document. No preamble, no JSON.
`

export function buildObserverConfig(settings: {
	observerEnabled: boolean
	observerProvider?: string
	observerModelId?: string
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
		tokenThreshold: settings.observerTokenThreshold,
		bufferActivation: settings.observerBufferActivation,
		blockAfter: settings.observerBlockAfter,
		reflectionEnabled: settings.observerReflectionEnabled,
		reflectionTokenThreshold: settings.observerReflectionTokenThreshold,
	}
}
