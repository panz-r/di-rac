/**
 * Declarative provider registry — single source of truth for provider metadata.
 *
 * All lookup maps (ProviderToApiKeyMap, model ID keys, default models,
 * isProviderConfigured, provider list) are auto-derived from this registry.
 * Adding a provider = add one descriptor entry here + handler + state-keys fields.
 */

import type { ApiProvider } from "../api"
import {
	anthropicDefaultModelId,
	basetenDefaultModelId,
	bedrockDefaultModelId,
	deepSeekDefaultModelId,
	fireworksDefaultModelId,
	geminiDefaultModelId,
	groqDefaultModelId,
	huaweiCloudMaasDefaultModelId,
	huggingFaceDefaultModelId,
	internationalQwenDefaultModelId,
	liteLlmDefaultModelId,
	minimaxDefaultModelId,
	moonshotDefaultModelId,
	nousResearchDefaultModelId,
	openAiNativeDefaultModelId,
	openRouterDefaultModelId,
	requestyDefaultModelId,
	sapAiCoreDefaultModelId,
	vertexDefaultModelId,
	wandbDefaultModelId,
	xaiDefaultModelId,
} from "../api"
import type { Secrets, SettingsKey } from "../storage/state-keys"

// ---------------------------------------------------------------------------
// Provider descriptor interface
// ---------------------------------------------------------------------------

export interface ProviderDescriptor {
	/** Unique provider identifier — matches ApiProvider union */
	providerId: ApiProvider
	/** Human-readable label for UI */
	label: string
	/**
	 * Model ID state key suffix. When set, model IDs are stored in
	 * `${mode}Mode${suffix}` (e.g., "OpenRouterModelId" → actModeOpenRouterModelId).
	 * When absent, the generic actModeApiModelId / planModeApiModelId is used.
	 */
	modelIdKeySuffix?: string
	/** Secret key name(s) in the secrets store */
	apiKeyFields?: keyof Secrets | (keyof Secrets)[]
	/** Hardcoded default model ID */
	defaultModelId?: string
	/**
	 * Custom "is configured" check. When absent, isProviderConfigured()
	 * checks whether any field in apiKeyFields is truthy.
	 */
	isConfiguredOverride?: (config: Record<string, unknown>) => boolean
}

// ---------------------------------------------------------------------------
// Provider registry — order defines UI display order
// ---------------------------------------------------------------------------

export const PROVIDER_REGISTRY: ProviderDescriptor[] = [
	{
		providerId: "dirac",
		label: "Dirac",
		modelIdKeySuffix: "DiracModelId",
		apiKeyFields: "diracApiKey",
		defaultModelId: openRouterDefaultModelId,
	},
	{
		providerId: "openai-codex",
		label: "ChatGPT Subscription",
		isConfiguredOverride: (c) => !!c["openai-codex-oauth-credentials"],
	},
	{
		providerId: "gemini",
		label: "Google Gemini",
		apiKeyFields: "geminiApiKey",
		defaultModelId: geminiDefaultModelId,
	},
	{
		providerId: "openai",
		label: "OpenAI Compatible",
		modelIdKeySuffix: "OpenAiModelId",
		apiKeyFields: ["openAiApiKey", "openAiCompatibleCustomApiKey"],
		defaultModelId: openAiNativeDefaultModelId,
		isConfiguredOverride: (c) =>
			!!(
				(c.openAiBaseUrl && (c.openAiApiKey || c.openAiCompatibleCustomApiKey)) ||
				c.planModeOpenAiModelId ||
				c.actModeOpenAiModelId
			),
	},
	{
		providerId: "anthropic",
		label: "Anthropic",
		apiKeyFields: "apiKey",
		defaultModelId: anthropicDefaultModelId,
	},
	{
		providerId: "bedrock",
		label: "Amazon Bedrock",
		apiKeyFields: ["awsAccessKey", "awsBedrockApiKey"],
		defaultModelId: bedrockDefaultModelId,
		isConfiguredOverride: (c) => !!c.awsRegion,
	},
	{
		providerId: "vscode-lm",
		label: "GitHub Copilot",
	},
	{
		providerId: "deepseek",
		label: "DeepSeek",
		apiKeyFields: "deepSeekApiKey",
		defaultModelId: deepSeekDefaultModelId,
	},
	{
		providerId: "openai-native",
		label: "OpenAI",
		apiKeyFields: "openAiNativeApiKey",
		defaultModelId: openAiNativeDefaultModelId,
	},
	{
		providerId: "openrouter",
		label: "OpenRouter",
		modelIdKeySuffix: "OpenRouterModelId",
		apiKeyFields: "openRouterApiKey",
		defaultModelId: openRouterDefaultModelId,
	},
	{
		providerId: "vertex",
		label: "GCP Vertex AI",
		apiKeyFields: "geminiApiKey",
		defaultModelId: vertexDefaultModelId,
		isConfiguredOverride: (c) => !!(c.vertexProjectId && c.vertexRegion),
	},
	{
		providerId: "litellm",
		label: "LiteLLM",
		modelIdKeySuffix: "LiteLlmModelId",
		apiKeyFields: "liteLlmApiKey",
		defaultModelId: liteLlmDefaultModelId,
		isConfiguredOverride: (c) =>
			!!(c.liteLlmBaseUrl || c.liteLlmApiKey || c.planModeLiteLlmModelId || c.actModeLiteLlmModelId),
	},
	{
		providerId: "claude-code",
		label: "Claude Code",
		isConfiguredOverride: (c) => !!c.claudeCodePath,
	},
	{
		providerId: "sapaicore",
		label: "SAP AI Core",
		modelIdKeySuffix: "SapAiCoreModelId",
		apiKeyFields: ["sapAiCoreClientId", "sapAiCoreClientSecret"],
		defaultModelId: sapAiCoreDefaultModelId,
		isConfiguredOverride: (c) =>
			!!(c.sapAiCoreBaseUrl && c.sapAiCoreClientId && c.sapAiCoreClientSecret && c.sapAiCoreTokenUrl),
	},
	{
		providerId: "mistral",
		label: "Mistral",
		apiKeyFields: "mistralApiKey",
	},
	{
		providerId: "zai",
		label: "Z AI",
		apiKeyFields: "zaiApiKey",
	},
	{
		providerId: "groq",
		label: "Groq",
		modelIdKeySuffix: "GroqModelId",
		apiKeyFields: "groqApiKey",
		defaultModelId: groqDefaultModelId,
	},
	{
		providerId: "cerebras",
		label: "Cerebras",
		apiKeyFields: "cerebrasApiKey",
	},
	{
		providerId: "vercel-ai-gateway",
		label: "Vercel AI Gateway",
		modelIdKeySuffix: "VercelAiGatewayModelId",
		apiKeyFields: "vercelAiGatewayApiKey",
		defaultModelId: openRouterDefaultModelId,
	},
	{
		providerId: "baseten",
		label: "Baseten",
		modelIdKeySuffix: "BasetenModelId",
		apiKeyFields: "basetenApiKey",
		defaultModelId: basetenDefaultModelId,
	},
	{
		providerId: "requesty",
		label: "Requesty",
		modelIdKeySuffix: "RequestyModelId",
		apiKeyFields: "requestyApiKey",
		defaultModelId: requestyDefaultModelId,
	},
	{
		providerId: "fireworks",
		label: "Fireworks AI",
		modelIdKeySuffix: "FireworksModelId",
		apiKeyFields: "fireworksApiKey",
		defaultModelId: fireworksDefaultModelId,
	},
	{
		providerId: "together",
		label: "Together",
		modelIdKeySuffix: "TogetherModelId",
		apiKeyFields: "togetherApiKey",
		defaultModelId: openRouterDefaultModelId,
	},
	{
		providerId: "qwen",
		label: "Alibaba Qwen",
		apiKeyFields: "qwenApiKey",
		defaultModelId: internationalQwenDefaultModelId,
	},
	{
		providerId: "qwen-code",
		label: "Qwen Code",
		apiKeyFields: "qwenApiKey",
	},
	{
		providerId: "doubao",
		label: "Bytedance Doubao",
		apiKeyFields: "doubaoApiKey",
	},
	{
		providerId: "lmstudio",
		label: "LM Studio",
		modelIdKeySuffix: "LmStudioModelId",
		defaultModelId: "",
		isConfiguredOverride: (c) =>
			!!(c.lmStudioBaseUrl || c.planModeLmStudioModelId || c.actModeLmStudioModelId),
	},
	{
		providerId: "moonshot",
		label: "Moonshot",
		apiKeyFields: "moonshotApiKey",
		defaultModelId: moonshotDefaultModelId,
	},
	{
		providerId: "huggingface",
		label: "Hugging Face",
		modelIdKeySuffix: "HuggingFaceModelId",
		apiKeyFields: "huggingFaceApiKey",
		defaultModelId: huggingFaceDefaultModelId,
	},
	{
		providerId: "nebius",
		label: "Nebius AI Studio",
		apiKeyFields: "nebiusApiKey",
	},
	{
		providerId: "asksage",
		label: "AskSage",
		apiKeyFields: "asksageApiKey",
	},
	{
		providerId: "xai",
		label: "xAI",
		apiKeyFields: "xaiApiKey",
		defaultModelId: xaiDefaultModelId,
	},
	{
		providerId: "sambanova",
		label: "SambaNova",
		apiKeyFields: "sambanovaApiKey",
	},
	{
		providerId: "huawei-cloud-maas",
		label: "Huawei Cloud MaaS",
		modelIdKeySuffix: "HuaweiCloudMaasModelId",
		apiKeyFields: "huaweiCloudMaasApiKey",
		defaultModelId: huaweiCloudMaasDefaultModelId,
	},
	{
		providerId: "dify",
		label: "Dify.ai",
		apiKeyFields: "difyApiKey",
		isConfiguredOverride: (c) => !!(c.difyBaseUrl && c.difyApiKey),
	},
	{
		providerId: "oca",
		label: "Oracle Code Assist",
	},
	{
		providerId: "minimax",
		label: "MiniMax",
		apiKeyFields: "minimaxApiKey",
		defaultModelId: minimaxDefaultModelId,
	},
	{
		providerId: "hicap",
		label: "Hicap",
		modelIdKeySuffix: "HicapModelId",
		apiKeyFields: "hicapApiKey",
		defaultModelId: "",
	},
	{
		providerId: "aihubmix",
		label: "AIhubmix",
		modelIdKeySuffix: "AihubmixModelId",
		apiKeyFields: "aihubmixApiKey",
		defaultModelId: openRouterDefaultModelId,
	},
	{
		providerId: "nousResearch",
		label: "NousResearch",
		modelIdKeySuffix: "NousResearchModelId",
		apiKeyFields: "nousResearchApiKey",
		defaultModelId: nousResearchDefaultModelId,
	},
	{
		providerId: "wandb",
		label: "W&B Inference by CoreWeave",
		apiKeyFields: "wandbApiKey",
		defaultModelId: wandbDefaultModelId,
	},
	{
		providerId: "nvidia-nim",
		label: "NVIDIA NIM",
		modelIdKeySuffix: "NvidiaNimModelId",
		apiKeyFields: "nvidiaNimApiKey",
		defaultModelId: "nvidia/llama-3.1-nemotron-ultra-253b-v1",
		isConfiguredOverride: (c) =>
			!!(c.nvidiaNimApiKey || c.nvidiaNimBaseUrl || c.planModeNvidiaNimModelId || c.actModeNvidiaNimModelId),
	},
]

// ---------------------------------------------------------------------------
// Derived lookups — auto-built from the registry
// ---------------------------------------------------------------------------

const byId = new Map(PROVIDER_REGISTRY.map((d) => [d.providerId, d]))

/** Provider list for UI dropdowns (replaces providers.json) */
export const PROVIDER_LIST: readonly { value: string; label: string }[] = PROVIDER_REGISTRY.map((d) => ({
	value: d.providerId,
	label: d.label,
}))

/** Map provider → API key secret field(s) (backward-compatible) */
export const ProviderToApiKeyMap: Partial<Record<ApiProvider, keyof Secrets | (keyof Secrets)[]>> =
	Object.fromEntries(
		PROVIDER_REGISTRY.filter((d) => d.apiKeyFields).map((d) => [d.providerId, d.apiKeyFields!]),
	)

/**
 * Get the provider-specific model ID state key for a given provider and mode.
 * Providers with a modelIdKeySuffix get dedicated keys (e.g., actModeOpenRouterModelId).
 * Others fall back to the generic actModeApiModelId / planModeApiModelId.
 */
export function getProviderModelIdKey(provider: ApiProvider, mode: "act" | "plan"): SettingsKey {
	const descriptor = byId.get(provider)
	if (descriptor?.modelIdKeySuffix) {
		return `${mode}Mode${descriptor.modelIdKeySuffix}` as SettingsKey
	}
	return `${mode}ModeApiModelId` as SettingsKey
}

/** Get the default model ID for a provider. */
export function getProviderDefaultModelId(provider: ApiProvider): string {
	return byId.get(provider)?.defaultModelId || ""
}

/**
 * Check if a provider has the required credentials/settings configured.
 * Uses isConfiguredOverride if provided, otherwise checks apiKeyFields.
 */
export function isProviderConfigured(providerId: string, config: unknown): boolean {
	const descriptor = byId.get(providerId as ApiProvider)
	if (!descriptor) return false

	const c = config as Record<string, unknown>

	if (descriptor.isConfiguredOverride) {
		return descriptor.isConfiguredOverride(c)
	}

	if (descriptor.apiKeyFields) {
		const fields = Array.isArray(descriptor.apiKeyFields) ? descriptor.apiKeyFields : [descriptor.apiKeyFields]
		return fields.some((f) => !!c[f])
	}

	return false
}
