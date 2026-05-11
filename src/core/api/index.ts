import { ApiConfiguration, ModelInfo, ALL_MODEL_MAPS } from "@shared/api"
import { getSettingsForMode } from "@shared/storage/provider-settings"
import { Mode } from "@shared/storage/types"
import { getRoleStateKey } from "@shared/roles"
import type { ModelRole } from "@shared/roles"
import { DiracStorageMessage } from "@/shared/messages/content"
import { Logger } from "@/shared/services/Logger"
import { DiracTool } from "@/shared/tools"
import { ApiGatewayHandler } from "./providers/api-gateway"
import { ApiStream, ApiStreamUsageChunk } from "./transform/stream"

export type CommonApiHandlerOptions = {
	onRetryAttempt?: ApiConfiguration["onRetryAttempt"]
}
export interface ApiHandler {
	createMessage(systemPrompt: string, messages: DiracStorageMessage[], tools?: DiracTool[], useResponseApi?: boolean): ApiStream
	getModel(): ApiHandlerModel
	getApiStreamUsage?(): Promise<ApiStreamUsageChunk | undefined>
	abort?(): void
}

export interface ApiHandlerModel {
	id: string
	info: ModelInfo
}

export interface ApiProviderInfo {
	providerId: string
	model: ApiHandlerModel
	mode: ModelRole
	customPrompt?: string // "compact"
}

export interface SingleCompletionHandler {
	completePrompt(prompt: string): Promise<string>
}

// Extract provider-mode settings from the flat providerSettings map.
function extractProviderSettings(
	store: Record<string, unknown>,
	providerId: string,
	mode: "act" | "plan",
): Record<string, unknown> {
	const result: Record<string, unknown> = {}
	const globalPrefix = `${providerId}:global:`
	const modePrefix = `${providerId}:${mode}:`
	for (const [key, value] of Object.entries(store)) {
		if (key.startsWith(modePrefix)) {
			const settingKey = key.slice(modePrefix.length)
			result[settingKey] = value
		} else if (key.startsWith(globalPrefix)) {
			const settingKey = key.slice(globalPrefix.length)
			if (!(settingKey in result)) {
				result[settingKey] = value
			}
		}
	}
	return result
}

// Resolve ModelInfo for a model ID by searching all static model maps.
function resolveModelInfo(modelId: string | undefined): ModelInfo | undefined {
	if (!modelId) return undefined
	for (const [, map] of ALL_MODEL_MAPS) {
		if (modelId in map) return map[modelId]
	}
	return undefined
}

// Helper: create an ApiGatewayHandler for a provider that has been migrated to the Go gateway.
function gatewayHandler(
	providerId: string,
	opts: {
		apiKey?: string
		baseUrl?: string
		model?: string
		thinkingBudgetTokens?: number
		reasoningEffort?: string
		settings?: Record<string, unknown>
		modelInfo?: ModelInfo
	},
): ApiGatewayHandler {
	const modelInfo = opts.modelInfo || resolveModelInfo(opts.model)
	return new ApiGatewayHandler({
		providerId,
		apiKey: opts.apiKey,
		baseUrl: opts.baseUrl,
		model: opts.model,
		modelInfo,
		thinkingBudgetTokens: opts.thinkingBudgetTokens,
		enableThinking: modelInfo?.supportsReasoning === true,
		reasoningEffort: opts.reasoningEffort,
		settings: opts.settings,
	})
}

function createHandlerForProvider(
	apiProvider: string | undefined,
	options: Omit<ApiConfiguration, "apiProvider">,
	mode: "act" | "plan",
): ApiHandler {
	const thinkingBudgetTokens =
		mode === "plan" ? options.planModeThinkingBudgetTokens : options.actModeThinkingBudgetTokens
	const reasoningEffort =
		mode === "plan" ? options.planModeReasoningEffort : options.actModeReasoningEffort
	const providerSettingsStore = (options.providerSettings || {}) as Record<string, unknown>
	const providerSettings = extractProviderSettings(providerSettingsStore, apiProvider || "", mode)

	switch (apiProvider) {
		case "anthropic":
			return gatewayHandler("anthropic", {
				apiKey: options.apiKey,
				baseUrl: options.anthropicBaseUrl,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				thinkingBudgetTokens,
				settings: providerSettings,
			})
		case "openrouter":
			return gatewayHandler("openrouter", {
				apiKey: options.openRouterApiKey,
				model: mode === "plan" ? options.planModeOpenRouterModelId : options.actModeOpenRouterModelId,
				thinkingBudgetTokens,
				settings: providerSettings,
			})
		case "openai": {
			const openAiModelId = mode === "plan" ? options.planModeOpenAiModelId : options.actModeOpenAiModelId
			const apiKey = options.openAiCompatibleCustomApiKey || options.openAiApiKey
			return gatewayHandler("openai", {
				apiKey,
				baseUrl: options.openAiBaseUrl,
				model: openAiModelId,
				settings: providerSettings,
			})
		}
		case "lmstudio":
			return gatewayHandler("lmstudio", {
				baseUrl: options.lmStudioBaseUrl,
				model: mode === "plan" ? options.planModeLmStudioModelId : options.actModeLmStudioModelId,
				settings: providerSettings,
			})
		case "gemini":
			return gatewayHandler("gemini", {
				apiKey: options.geminiApiKey,
				baseUrl: options.geminiBaseUrl,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				thinkingBudgetTokens,
				settings: providerSettings,
			})
		case "deepseek":
			return gatewayHandler("deepseek", {
				apiKey: options.deepSeekApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				reasoningEffort,
				settings: providerSettings,
			})
		case "fireworks":
			return gatewayHandler("fireworks", {
				apiKey: options.fireworksApiKey,
				model: mode === "plan" ? options.planModeFireworksModelId : options.actModeFireworksModelId,
				settings: providerSettings,
			})
		case "together":
			return gatewayHandler("together", {
				apiKey: options.togetherApiKey,
				model: mode === "plan" ? options.planModeTogetherModelId : options.actModeTogetherModelId,
				settings: providerSettings,
			})
		case "qwen":
			return gatewayHandler("qwen", {
				apiKey: options.qwenApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				thinkingBudgetTokens,
				settings: providerSettings,
			})
		case "mistral":
			return gatewayHandler("mistral", {
				apiKey: options.mistralApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				settings: providerSettings,
			})
		case "moonshot":
			return gatewayHandler("moonshot", {
				apiKey: options.moonshotApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				settings: providerSettings,
			})
		case "huggingface":
			return gatewayHandler("huggingface", {
				apiKey: options.huggingFaceApiKey,
				model: mode === "plan" ? options.planModeHuggingFaceModelId : options.actModeHuggingFaceModelId,
				settings: providerSettings,
			})
		case "nebius":
			return gatewayHandler("nebius", {
				apiKey: options.nebiusApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				settings: providerSettings,
			})
		case "xai":
			return gatewayHandler("xai", {
				apiKey: options.xaiApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				settings: providerSettings,
			})
		case "sambanova":
			return gatewayHandler("sambanova", {
				apiKey: options.sambanovaApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				settings: providerSettings,
			})
		case "cerebras":
			return gatewayHandler("cerebras", {
				apiKey: options.cerebrasApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				settings: providerSettings,
			})
		case "groq":
			return gatewayHandler("groq", {
				apiKey: options.groqApiKey,
				model: mode === "plan" ? options.planModeGroqModelId : options.actModeGroqModelId,
				settings: providerSettings,
			})
		case "zai":
			return gatewayHandler("zai", {
				apiKey: options.zaiApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				thinkingBudgetTokens,
				settings: providerSettings,
			})
		case "minimax":
			return gatewayHandler("minimax", {
				apiKey: options.minimaxApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				thinkingBudgetTokens,
				settings: providerSettings,
			})
		case "nvidia-nim":
			return gatewayHandler("nvidia-nim", {
				apiKey: options.nvidiaNimApiKey,
				baseUrl: options.nvidiaNimBaseUrl,
				model: mode === "plan" ? options.planModeNvidiaNimModelId : options.actModeNvidiaNimModelId,
				settings: providerSettings,
			})
		case "opencode_go":
			return gatewayHandler("opencode_go", {
				apiKey: options.openCodeGoApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				settings: providerSettings,
			})
		case "opencode_zen":
			return gatewayHandler("opencode_zen", {
				apiKey: options.openCodeZenApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				settings: providerSettings,
			})
		case "kilocode":
			return gatewayHandler("kilocode", {
				apiKey: options.kiloCodeApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				settings: providerSettings,
			})
		case "byteplus":
			return gatewayHandler("byteplus", {
				apiKey: options.byteplusApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				settings: providerSettings,
			})
		case "byteplus_coding_plan":
			return gatewayHandler("byteplus_coding_plan", {
				apiKey: options.byteplusApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				settings: providerSettings,
			})
		case "openai_codex":
			return gatewayHandler("openai_codex", {
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				settings: providerSettings,
			})
		case "xiaomi_mimo":
			return gatewayHandler("xiaomi_mimo", {
				apiKey: options.xiaomiMimoApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				settings: providerSettings,
			})
		case "synthetic":
			return gatewayHandler("synthetic", {
				apiKey: options.syntheticApiKey,
				model: mode === "plan" ? options.planModeSyntheticModelId : options.actModeSyntheticModelId,
				settings: providerSettings,
			})
		case "wafer":
			return gatewayHandler("wafer", {
				apiKey: options.waferApiKey,
				model: mode === "plan" ? options.planModeWaferModelId : options.actModeWaferModelId,
				thinkingBudgetTokens,
				settings: providerSettings,
			})
		case "venice":
			return gatewayHandler("venice", {
				apiKey: options.veniceApiKey,
				model: mode === "plan" ? options.planModeVeniceModelId : options.actModeVeniceModelId,
				settings: providerSettings,
			})
		case "inference_net":
			return gatewayHandler("inference_net", {
				apiKey: options.inferenceNetApiKey,
				model: mode === "plan" ? options.planModeInferenceNetModelId : options.actModeInferenceNetModelId,
				settings: providerSettings,
			})
		case "ovhcloud":
			return gatewayHandler("ovhcloud", {
				apiKey: options.ovhcloudApiKey,
				model: mode === "plan" ? options.planModeOvhcloudModelId : options.actModeOvhcloudModelId,
				settings: providerSettings,
			})
		case "ollama":
			return gatewayHandler("ollama", {
				apiKey: options.ollamaApiKey,
				model: mode === "plan" ? options.planModeOllamaModelId : options.actModeOllamaModelId,
				thinkingBudgetTokens,
				settings: providerSettings,
			})
		case "replicate":
			return gatewayHandler("replicate", {
				apiKey: options.replicateApiKey,
				model: mode === "plan" ? options.planModeReplicateModelId : options.actModeReplicateModelId,
				settings: providerSettings,
			})
		case "api-gateway": {
			const gwModel = mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId
			const gwModelInfo = resolveModelInfo(gwModel)
			return new ApiGatewayHandler({
				providerId: options.apiGatewayProviderId || "anthropic",
				apiKey: options.apiGatewayApiKey,
				baseUrl: options.apiGatewayBaseUrl,
				model: gwModel,
				modelInfo: gwModelInfo,
				thinkingBudgetTokens,
				enableThinking: gwModelInfo?.supportsReasoning === true,
				settings: providerSettings,
			})
		}

		default:
			throw new Error(`No provider set. Current provider: "${apiProvider || "unknown"}". Configure a provider in settings.`)
	}
}

export function buildApiHandler(configuration: ApiConfiguration, role: ModelRole): ApiHandler {
	const { planModeApiProvider, actModeApiProvider, ...options } = configuration as Record<string, any>

	// Resolve effective mode and provider.
	// Act/plan use their dedicated provider keys.
	// Other roles (observe, etc.) read a role-specific provider key, falling back to act.
	let effectiveMode: "act" | "plan"
	let apiProvider: string | undefined

	if (role === "plan") {
		effectiveMode = "plan"
		apiProvider = planModeApiProvider || actModeApiProvider
	} else if (role === "act") {
		effectiveMode = "act"
		apiProvider = actModeApiProvider
	} else {
		// Non-act/plan role (observe, future roles)
		const cfg = configuration as Record<string, any>
		const roleProviderKey = getRoleStateKey(role, "provider")
		apiProvider = cfg[roleProviderKey] || actModeApiProvider

		if (cfg[roleProviderKey]) {
			const roleModelKey = getRoleStateKey(role, "apiModelId")
			options.actModeApiModelId = cfg[roleModelKey] || options.actModeApiModelId
			options.actModeApiProvider = cfg[roleProviderKey]
		}
		effectiveMode = "act"
	}

	// Validate thinking budget tokens against model's maxTokens to prevent API errors
	try {
		const thinkingBudgetTokens = effectiveMode === "plan" ? options.planModeThinkingBudgetTokens : options.actModeThinkingBudgetTokens
		if (thinkingBudgetTokens && thinkingBudgetTokens > 0) {
			const handler = createHandlerForProvider(apiProvider, options, effectiveMode)

			const modelInfo = handler.getModel().info
			if (modelInfo?.maxTokens && modelInfo.maxTokens > 0 && thinkingBudgetTokens > modelInfo.maxTokens) {
				const clippedValue = modelInfo.maxTokens - 1
				if (effectiveMode === "plan") {
					options.planModeThinkingBudgetTokens = clippedValue
				} else {
					options.actModeThinkingBudgetTokens = clippedValue
				}
			} else {
				return handler
			}
		}
	} catch (error) {
		Logger.error("buildApiHandler error:", error)
	}

	return createHandlerForProvider(apiProvider, options, effectiveMode)
}
