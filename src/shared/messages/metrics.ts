import { Mode } from "../storage/types"

export interface DiracMessageModelInfo {
	modelId: string
	providerId: string
	mode: Mode
}

interface DiracTokensInfo {
	prompt: number // Total input tokens (includes cached + non-cached)
	completion: number // Total output tokens
	reasoning?: number // Subset of completion_tokens that were reasoning tokens
	cached: number // Subset of prompt_tokens that were cache hits
}

export interface DiracMessageMetricsInfo {
	tokens?: DiracTokensInfo
}
