import { ApiConfiguration, ModelInfo, openAiModelInfoSaneDefaults, QwenApiRegions } from "@shared/api"
import { queryProviderInfo, type ProviderSetting } from "@/core/api/providers/api-gateway"
import { getSettingsForMode } from "@shared/storage/provider-settings" 
import { Mode } from "@shared/storage/types"
import { getRoleStateKey } from "@shared/roles"
import type { ModelRole } from "@shared/roles"
import { DiracStorageMessage } from "@/shared/messages/content"
import { Logger } from "@/shared/services/Logger"
import { DiracTool } from "@/shared/tools"
import { AIhubmixHandler } from "./providers/aihubmix"
import { ApiGatewayHandler } from "./providers/api-gateway"
import { AskSageHandler } from "./providers/asksage"
import { BasetenHandler } from "./providers/baseten"
import { AwsBedrockHandler } from "./providers/bedrock"
import { DifyHandler } from "./providers/dify"
import { OpenAiCodexHandler } from "./providers/openai-codex"
import { OpenAiNativeHandler } from "./providers/openai-native"
import { QwenCodeHandler } from "./providers/qwen-code"
import { SapAiCoreHandler } from "./providers/sapaicore"
import { VertexHandler } from "./providers/vertex"
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
	},
): ApiGatewayHandler {
	return new ApiGatewayHandler({
		providerId,
		apiKey: opts.apiKey,
		baseUrl: opts.baseUrl,
		model: opts.model,
		thinkingBudgetTokens: opts.thinkingBudgetTokens,
		enableThinking: true,
		reasoningEffort: opts.reasoningEffort,
		settings: opts.settings,
	})
}

function createHandlerForProvider(
	apiProvider: string | undefined,
	options: Omit<ApiConfiguration, "apiProvider">,
	mode: "act" | "plan",
): ApiHandler {
	// Providers migrated to the Go API gateway
	const thinkingBudgetTokens =
		mode === "plan" ? options.planModeThinkingBudgetTokens : options.actModeThinkingBudgetTokens
	const reasoningEffort =
		mode === "plan" ? options.planModeReasoningEffort : options.actModeReasoningEffort
	const providerSettingsStore = (options.providerSettings || {}) as Record<string, unknown>
	const providerSettings = extractProviderSettings(providerSettingsStore, apiProvider || "", mode)

	switch (apiProvider) {
		// --- Go gateway providers ---
		case "anthropic":
			return gatewayHandler("anthropic", {
				apiKey: options.apiKey,
				baseUrl: options.anthropicBaseUrl,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				thinkingBudgetTokens,
			})
		case "openrouter":
			return gatewayHandler("openrouter", {
				apiKey: options.openRouterApiKey,
				model: mode === "plan" ? options.planModeOpenRouterModelId : options.actModeOpenRouterModelId,
				thinkingBudgetTokens,
			})
		case "openai": {
			const openAiModelId = mode === "plan" ? options.planModeOpenAiModelId : options.actModeOpenAiModelId
			const apiKey = options.openAiCompatibleCustomApiKey || options.openAiApiKey
			return gatewayHandler("openai", {
				apiKey,
				baseUrl: options.openAiBaseUrl,
				model: openAiModelId,
			})
		}
		case "lmstudio":
			return gatewayHandler("lmstudio", {
				baseUrl: options.lmStudioBaseUrl,
				model: mode === "plan" ? options.planModeLmStudioModelId : options.actModeLmStudioModelId,
			})
		case "gemini":
			return gatewayHandler("gemini", {
				apiKey: options.geminiApiKey,
				baseUrl: options.geminiBaseUrl,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				thinkingBudgetTokens,
			})
		case "deepseek":
			return gatewayHandler("deepseek", {
				apiKey: options.deepSeekApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				reasoningEffort,
				settings: providerSettings,
			})
		case "requesty":
			return gatewayHandler("requesty", {
				apiKey: options.requestyApiKey,
				baseUrl: options.requestyBaseUrl,
				model: mode === "plan" ? options.planModeRequestyModelId : options.actModeRequestyModelId,
				thinkingBudgetTokens,
			})
		case "fireworks":
			return gatewayHandler("fireworks", {
				apiKey: options.fireworksApiKey,
				model: mode === "plan" ? options.planModeFireworksModelId : options.actModeFireworksModelId,
			})
		case "together":
			return gatewayHandler("together", {
				apiKey: options.togetherApiKey,
				model: mode === "plan" ? options.planModeTogetherModelId : options.actModeTogetherModelId,
			})
		case "qwen":
			return gatewayHandler("qwen", {
				apiKey: options.qwenApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				thinkingBudgetTokens,
			})
		case "doubao":
			return gatewayHandler("doubao", {
				apiKey: options.doubaoApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
			})
		case "mistral":
			return gatewayHandler("mistral", {
				apiKey: options.mistralApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
			})
		case "litellm":
			return gatewayHandler("litellm", {
				apiKey: options.liteLlmApiKey,
				baseUrl: options.liteLlmBaseUrl,
				model: mode === "plan" ? options.planModeLiteLlmModelId : options.actModeLiteLlmModelId,
				thinkingBudgetTokens,
			})
		case "moonshot":
			return gatewayHandler("moonshot", {
				apiKey: options.moonshotApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
			})
		case "huggingface":
			return gatewayHandler("huggingface", {
				apiKey: options.huggingFaceApiKey,
				model: mode === "plan" ? options.planModeHuggingFaceModelId : options.actModeHuggingFaceModelId,
			})
		case "nebius":
			return gatewayHandler("nebius", {
				apiKey: options.nebiusApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
			})
		case "xai":
			return gatewayHandler("xai", {
				apiKey: options.xaiApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
			})
		case "sambanova":
			return gatewayHandler("sambanova", {
				apiKey: options.sambanovaApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
			})
		case "cerebras":
			return gatewayHandler("cerebras", {
				apiKey: options.cerebrasApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
			})
		case "groq":
			return gatewayHandler("groq", {
				apiKey: options.groqApiKey,
				model: mode === "plan" ? options.planModeGroqModelId : options.actModeGroqModelId,
			})
		case "huawei-cloud-maas":
			return gatewayHandler("huawei-cloud-maas", {
				apiKey: options.huaweiCloudMaasApiKey,
				model: mode === "plan" ? options.planModeHuaweiCloudMaasModelId : options.actModeHuaweiCloudMaasModelId,
			})
		case "vercel-ai-gateway":
			return gatewayHandler("vercel-ai-gateway", {
				apiKey: options.vercelAiGatewayApiKey,
				model: mode === "plan" ? options.planModeVercelAiGatewayModelId : options.actModeVercelAiGatewayModelId,
				thinkingBudgetTokens,
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
			})
		case "hicap":
			return gatewayHandler("hicap", {
				apiKey: options.hicapApiKey,
				model: mode === "plan" ? options.planModeHicapModelId : options.actModeHicapModelId,
			})
		case "nousResearch":
			return gatewayHandler("nousresearch", {
				apiKey: options.nousResearchApiKey,
				model: mode === "plan" ? options.planModeNousResearchModelId : options.actModeNousResearchModelId,
			})
		case "wandb":
			return gatewayHandler("wandb", {
				apiKey: options.wandbApiKey,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
			})
		case "nvidia-nim":
			return gatewayHandler("nvidia-nim", {
				apiKey: options.nvidiaNimApiKey,
				baseUrl: options.nvidiaNimBaseUrl,
				model: mode === "plan" ? options.planModeNvidiaNimModelId : options.actModeNvidiaNimModelId,
			})
		case "api-gateway":
			return new ApiGatewayHandler({
				providerId: options.apiGatewayProviderId || "anthropic",
				apiKey: options.apiGatewayApiKey,
				baseUrl: options.apiGatewayBaseUrl,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				thinkingBudgetTokens,
				enableThinking: true,
			})

		// --- TypeScript-only providers (not yet migrated to Go gateway) ---
		case "bedrock":
			return new AwsBedrockHandler({
				onRetryAttempt: options.onRetryAttempt,
				apiModelId: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				awsAccessKey: options.awsAccessKey,
				awsSecretKey: options.awsSecretKey,
				awsSessionToken: options.awsSessionToken,
				awsRegion: options.awsRegion,
				awsAuthentication: options.awsAuthentication,
				awsBedrockApiKey: options.awsBedrockApiKey,
				awsUseCrossRegionInference: options.awsUseCrossRegionInference,
				awsUseGlobalInference: options.awsUseGlobalInference,
				awsBedrockUsePromptCache: options.awsBedrockUsePromptCache,
				awsUseProfile: options.awsUseProfile,
				awsProfile: options.awsProfile,
				awsBedrockEndpoint: options.awsBedrockEndpoint,
				awsBedrockCustomSelected:
					mode === "plan" ? options.planModeAwsBedrockCustomSelected : options.actModeAwsBedrockCustomSelected,
				awsBedrockCustomModelBaseId:
					mode === "plan" ? options.planModeAwsBedrockCustomModelBaseId : options.actModeAwsBedrockCustomModelBaseId,
				thinkingBudgetTokens,
			})
		case "vertex":
			return new VertexHandler({
				onRetryAttempt: options.onRetryAttempt,
				vertexProjectId: options.vertexProjectId,
				vertexRegion: options.vertexRegion,
				apiModelId: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				thinkingBudgetTokens,
				geminiApiKey: options.geminiApiKey,
				geminiBaseUrl: options.geminiBaseUrl,
				reasoningEffort: mode === "plan" ? options.planModeReasoningEffort : options.actModeReasoningEffort,
				ulid: options.ulid,
				geminiSearchEnabled: options.geminiSearchEnabled,
			})
		case "openai-native":
			return new OpenAiNativeHandler({
				onRetryAttempt: options.onRetryAttempt,
				openAiNativeApiKey: options.openAiNativeApiKey,
				reasoningEffort: mode === "plan" ? options.planModeReasoningEffort : options.actModeReasoningEffort,
				apiModelId: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				thinkingBudgetTokens,
			})
		case "openai-codex":
			return new OpenAiCodexHandler({
				onRetryAttempt: options.onRetryAttempt,
				reasoningEffort: mode === "plan" ? options.planModeReasoningEffort : options.actModeReasoningEffort,
				apiModelId: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
			})
		case "qwen-code":
			return new QwenCodeHandler({
				onRetryAttempt: options.onRetryAttempt,
				qwenCodeOauthPath: options.qwenCodeOauthPath,
				apiModelId: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
			})
		case "asksage":
			return new AskSageHandler({
				onRetryAttempt: options.onRetryAttempt,
				asksageApiKey: options.asksageApiKey,
				asksageApiUrl: options.asksageApiUrl,
				apiModelId: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
			})
		case "baseten":
			return new BasetenHandler({
				onRetryAttempt: options.onRetryAttempt,
				basetenApiKey: options.basetenApiKey,
				basetenModelId: mode === "plan" ? options.planModeBasetenModelId : options.actModeBasetenModelId,
				basetenModelInfo: mode === "plan" ? options.planModeBasetenModelInfo : options.actModeBasetenModelInfo,
				apiModelId: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
			})
		case "sapaicore":
			return new SapAiCoreHandler({
				onRetryAttempt: options.onRetryAttempt,
				sapAiCoreClientId: options.sapAiCoreClientId,
				sapAiCoreClientSecret: options.sapAiCoreClientSecret,
				sapAiCoreTokenUrl: options.sapAiCoreTokenUrl,
				sapAiResourceGroup: options.sapAiResourceGroup,
				sapAiCoreBaseUrl: options.sapAiCoreBaseUrl,
				apiModelId: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				thinkingBudgetTokens,
				reasoningEffort: mode === "plan" ? options.planModeReasoningEffort : options.actModeReasoningEffort,
				deploymentId: mode === "plan" ? options.planModeSapAiCoreDeploymentId : options.actModeSapAiCoreDeploymentId,
				sapAiCoreUseOrchestrationMode: options.sapAiCoreUseOrchestrationMode,
			})
		case "dify":
			return new DifyHandler({
				difyApiKey: options.difyApiKey,
				difyBaseUrl: options.difyBaseUrl,
			})
		case "aihubmix":
			return new AIhubmixHandler({
				onRetryAttempt: options.onRetryAttempt,
				apiKey: options.aihubmixApiKey,
				baseURL: options.aihubmixBaseUrl,
				appCode: options.aihubmixAppCode,
				modelId: mode === "plan" ? (options as any).planModeAihubmixModelId : (options as any).actModeAihubmixModelId,
				modelInfo:
					mode === "plan" ? (options as any).planModeAihubmixModelInfo : (options as any).actModeAihubmixModelInfo,
			})
		default:
			return gatewayHandler("anthropic", {
				apiKey: options.apiKey,
				baseUrl: options.anthropicBaseUrl,
				model: mode === "plan" ? options.planModeApiModelId : options.actModeApiModelId,
				thinkingBudgetTokens,
			})
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
