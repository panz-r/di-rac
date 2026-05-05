/**
 * Stub module for OpenRouter model-related functionality.
 * Model list caching has been removed - api-gateway now handles model fetching.
 * This module is kept as a stub to satisfy existing imports.
 */
import type { ModelInfo } from "@shared/api"
import type { Controller } from ".."

/**
 * Stealth models are models that are compatible with the OpenRouter API
 * but not listed on the OpenRouter website or API.
 */
const CLINE_STEALTH_MODELS: Record<string, ModelInfo> = {
	"stealth/giga-potato": {
		name: "Giga Potato",
		maxTokens: 8192,
		contextWindow: 224_000,
		supportsImages: true,
		supportsPromptCache: true,
		inputPrice: 0,
		outputPrice: 0,
		description: "A stealth model for testing purposes. Not a real potato.",
	},
}

/**
 * Append stealth models to the model list.
 * This function is kept for backwards compatibility.
 */
export function appendDiracStealthModels(currentModels: Record<string, ModelInfo>): Record<string, ModelInfo> {
	const cloned = { ...currentModels }
	for (const [modelId, modelInfo] of Object.entries(CLINE_STEALTH_MODELS)) {
		if (!cloned[modelId]) {
			cloned[modelId] = modelInfo
		}
	}
	return cloned
}

/**
 * Stub refresh function - model caching is removed.
 * Returns an empty record since api-gateway handles model fetching.
 */
export async function refreshOpenRouterModels(_controller: Controller): Promise<Record<string, ModelInfo>> {
	return {}
}
