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
	deepSeekDefaultModelId,
	fireworksDefaultModelId,
	groqDefaultModelId,
	huggingFaceDefaultModelId,
	internationalQwenDefaultModelId,
	minimaxDefaultModelId,
	moonshotDefaultModelId,
	openRouterDefaultModelId,
	xaiDefaultModelId,
	syntheticDefaultModelId,
	waferDefaultModelId,
} from "../api"
import type { ModelRole } from "../roles"
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
		providerId: "gemini",
		label: "Google Gemini",
		apiKeyFields: "geminiApiKey",
		defaultModelId: "gemini-3-flash-preview",
	},
	{
		providerId: "openai",
		label: "OpenAI Compatible",
		modelIdKeySuffix: "OpenAiModelId",
		apiKeyFields: ["openAiApiKey", "openAiCompatibleCustomApiKey"],
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
		providerId: "openrouter",
		label: "OpenRouter",
		modelIdKeySuffix: "OpenRouterModelId",
		apiKeyFields: "openRouterApiKey",
		defaultModelId: openRouterDefaultModelId,
	},
	{
		providerId: "claude-code",
		label: "Claude Code",
		isConfiguredOverride: (c) => !!c.claudeCodePath,
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
	},
	{
		providerId: "qwen",
		label: "Alibaba Qwen",
		apiKeyFields: "qwenApiKey",
		defaultModelId: internationalQwenDefaultModelId,
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
		providerId: "nvidia-nim",
		label: "NVIDIA NIM",
		modelIdKeySuffix: "NvidiaNimModelId",
		apiKeyFields: "nvidiaNimApiKey",
		defaultModelId: "nvidia/llama-3.1-nemotron-ultra-253b-v1",
		isConfiguredOverride: (c) =>
			!!(c.nvidiaNimApiKey || c.nvidiaNimBaseUrl || c.planModeNvidiaNimModelId || c.actModeNvidiaNimModelId),
	},
	{
		providerId: "opencode_go",
		label: "OpenCode Go",
		apiKeyFields: "openCodeGoApiKey",
		defaultModelId: "opencode-go/deepseek-v4-flash",
	},
	{
		providerId: "opencode_zen",
		label: "OpenCode Zen",
		apiKeyFields: "openCodeZenApiKey",
		defaultModelId: "opencode/gpt-4",
	},
	{
		providerId: "kilocode",
		label: "Kilo Code",
		apiKeyFields: "kiloCodeApiKey",
		defaultModelId: "anthropic/claude-3-7-sonnet",
	},
	{
		providerId: "byteplus",
		label: "BytePlus",
		apiKeyFields: "byteplusApiKey",
	},
	{
		providerId: "byteplus_coding_plan",
		label: "BytePlus Coding Plan",
		apiKeyFields: "byteplusApiKey",
	},
	{
		providerId: "openai_codex",
		label: "OpenAI Codex",
		defaultModelId: "gpt-5.3-codex",
		isConfiguredOverride: () => {
			// Codex uses OAuth — always show as available in the picker.
			// Actual auth status is checked by the gateway at request time.
			return true
		},
	},
	{
		providerId: "xiaomi_mimo",
		label: "Xiaomi MiMo",
		apiKeyFields: "xiaomiMimoApiKey",
		defaultModelId: "mimo-v2.5-pro",
	},
	{
		providerId: "synthetic",
		label: "Synthetic",
		modelIdKeySuffix: "SyntheticModelId",
		apiKeyFields: "syntheticApiKey",
		defaultModelId: syntheticDefaultModelId,
	},
	{
		providerId: "wafer",
		label: "Wafer",
		modelIdKeySuffix: "WaferModelId",
		apiKeyFields: "waferApiKey",
		defaultModelId: waferDefaultModelId,
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
 * Get the provider-specific model ID state key for a given provider and role.
 * For observe, returns the generic observerModelId key.
 * For act/plan, providers with a modelIdKeySuffix get dedicated keys.
 */
export function getProviderModelIdKey(provider: ApiProvider, role: ModelRole): SettingsKey {
	if (role === "observe") return "observerModelId" as SettingsKey
	const modePrefix = role === "plan" ? "planMode" : "actMode"
	const descriptor = byId.get(provider)
	if (descriptor?.modelIdKeySuffix) {
		return `${modePrefix}${descriptor.modelIdKeySuffix}` as SettingsKey
	}
	return `${modePrefix}ApiModelId` as SettingsKey
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
