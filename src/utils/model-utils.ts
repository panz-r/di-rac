import { ApiHandlerModel, ApiProviderInfo } from "@core/api"
import {
	AnthropicModelId,
	anthropicModels, getProviderForModel
} from "@/shared/api"

export { supportsReasoningEffortForModel } from "@shared/utils/reasoning-support"

const CLAUDE_VERSION_MATCH_REGEX = /[-_ ]([\d](?:\.[05])?)[-_ ]?/
export const GEMINI_MAX_OUTPUT_TOKENS = 32_768

export function modelDoesntSupportWebp(apiHandlerModel: ApiHandlerModel): boolean {
	const modelId = apiHandlerModel.id.toLowerCase()
	return modelId.includes("grok")
}

/**
 * Determines if reasoning content should be skipped for a given model
 * Currently skips reasoning for:
 * - Grok-4 models since they only display "thinking" without useful information
 * - Devstral models since they don't support reasoning_details field
 */
export function shouldSkipReasoningForModel(modelId?: string): boolean {
	if (!modelId) {
		return false
	}
	const provider = getProviderForModel(modelId)
	return provider === "xai" || modelId.includes("devstral")
}

export function isAnthropicModelId(modelId: string): modelId is AnthropicModelId {
	if (getProviderForModel(modelId) === "anthropic") {
		return true
	}
	const CLAUDE_MODELS = ["sonnet", "opus", "haiku"]
	return modelId in anthropicModels || CLAUDE_MODELS.some((substring) => modelId.includes(substring))
}

export function isGPT5(id: string): boolean {
	const modelId = normalize(id)
	return modelId.includes("gpt-5") || modelId.includes("gpt5")
}

export function isLocalModel(providerInfo: ApiProviderInfo): boolean {
	const localProviders = ["lmstudio"]
	return localProviders.includes(normalize(providerInfo.providerId))
}

/** * Check if parallel tool calling is enabled.
 * For this fork, we always enable parallel tool calling to support multiple tool uses per turn.
 */
export function isParallelToolCallingEnabled(enableParallelSetting: boolean, providerInfo: ApiProviderInfo): boolean {
	return true
}

function normalize(text: string): string {
	return text.trim().toLowerCase()
}
