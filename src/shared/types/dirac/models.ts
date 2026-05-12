// Replaces proto-generated dirac/models types with plain TypeScript.

import { Metadata } from "./common"

export enum ApiProvider {
	ANTHROPIC = 0,
	OPENROUTER = 1,
	BEDROCK = 2,
	VERTEX = 3,
	OPENAI = 4,
	LMSTUDIO = 6,
	GEMINI = 7,
	OPENAI_NATIVE = 8,
	REQUESTY = 9,
	TOGETHER = 10,
	DEEPSEEK = 11,
	QWEN = 12,
	DOUBAO = 13,
	MISTRAL = 14,
	VSCODE_LM = 15,
	DIRAC = 16,
	LITELLM = 17,
	NEBIUS = 18,
	FIREWORKS = 19,
	ASKSAGE = 20,
	XAI = 21,
	SAMBANOVA = 22,
	CEREBRAS = 23,
	GROQ = 24,
	SAPAICORE = 25,
	CLAUDE_CODE = 26,
	MOONSHOT = 27,
	HUGGINGFACE = 28,
	HUAWEI_CLOUD_MAAS = 29,
	BASETEN = 30,
	ZAI = 31,
	VERCEL_AI_GATEWAY = 32,
	QWEN_CODE = 33,
	DIFY = 34,
	MINIMAX = 36,
	HICAP = 37,
	AIHUBMIX = 38,
	NOUSRESEARCH = 39,
	OPENAI_CODEX = 40,
	WANDB = 41,
	OPENCODE_GO = 42,
	OPENCODE_ZEN = 43,
	KILOCODE = 44,
	BYTEPLUS = 45,
}

export enum ApiFormat {
	ANTHROPIC_CHAT = 0,
	GEMINI_CHAT = 1,
	OPENAI_CHAT = 2,
	R1_CHAT = 3,
	OPENAI_RESPONSES = 4,
	OPENAI_RESPONSES_WEBSOCKET_MODE = 5,
}

export interface LanguageModelChatSelector {
	vendor?: string
	family?: string
	version?: string
	id?: string
}
export const LanguageModelChatSelector = {
	create(o: Partial<LanguageModelChatSelector> = {}): LanguageModelChatSelector {
		return { ...o }
	},
}

export interface PriceTier {
	tokenLimit: number
	price: number
}
export const PriceTier = {
	create(o: Partial<PriceTier> = {}): PriceTier {
		return { tokenLimit: o.tokenLimit ?? 0, price: o.price ?? 0 }
	},
}

export interface ThinkingConfig {
	maxBudget?: number
	outputPrice?: number
	outputPriceTiers?: PriceTier[]
}
export const ThinkingConfig = {
	create(o: Partial<ThinkingConfig> = {}): ThinkingConfig {
		return { ...o }
	},
}

export interface ModelTier {
	contextWindow: number
	inputPrice?: number
	outputPrice?: number
	cacheWritesPrice?: number
	cacheReadsPrice?: number
}
export const ModelTier = {
	create(o: Partial<ModelTier> = {}): ModelTier {
		return { contextWindow: o.contextWindow ?? 0, ...o }
	},
}

export interface OpenRouterModelInfo {
	maxTokens?: number
	contextWindow?: number
	supportsImages?: boolean
	supportsPromptCache: boolean
	inputPrice?: number
	outputPrice?: number
	cacheWritesPrice?: number
	cacheReadsPrice?: number
	description?: string
	thinkingConfig?: ThinkingConfig
	supportsGlobalEndpoint?: boolean
	tiers?: ModelTier[]
	name?: string
	temperature?: number
	supportsReasoning?: boolean
	apiFormat?: ApiFormat
	supportsTools?: boolean
}
export const OpenRouterModelInfo = {
	create(o: Partial<OpenRouterModelInfo> = {}): OpenRouterModelInfo {
		return { supportsPromptCache: o.supportsPromptCache ?? false, ...o }
	},
}

export interface OpenRouterCompatibleModelInfo {
	models: Record<string, OpenRouterModelInfo>
}
export const OpenRouterCompatibleModelInfo = {
	create(o: Partial<OpenRouterCompatibleModelInfo> = {}): OpenRouterCompatibleModelInfo {
		return { models: o.models ?? {} }
	},
}

export interface OpenAiCompatibleModelInfo {
	maxTokens?: number
	contextWindow?: number
	supportsImages?: boolean
	supportsPromptCache: boolean
	inputPrice?: number
	outputPrice?: number
	thinkingConfig?: ThinkingConfig
	supportsGlobalEndpoint?: boolean
	cacheWritesPrice?: number
	cacheReadsPrice?: number
	description?: string
	tiers?: ModelTier[]
	temperature?: number
	isR1FormatRequired?: boolean
	apiFormat?: ApiFormat
	supportsTools?: boolean
}
export const OpenAiCompatibleModelInfo = {
	create(o: Partial<OpenAiCompatibleModelInfo> = {}): OpenAiCompatibleModelInfo {
		return { supportsPromptCache: o.supportsPromptCache ?? false, ...o }
	},
}

export interface LiteLLMModelInfo {
	maxTokens?: number
	contextWindow?: number
	supportsImages?: boolean
	supportsPromptCache: boolean
	inputPrice?: number
	outputPrice?: number
	thinkingConfig?: ThinkingConfig
	supportsGlobalEndpoint?: boolean
	cacheWritesPrice?: number
	cacheReadsPrice?: number
	description?: string
	tiers?: ModelTier[]
	temperature?: number
	apiFormat?: ApiFormat
	supportsReasoning?: boolean
}
export const LiteLLMModelInfo = {
	create(o: Partial<LiteLLMModelInfo> = {}): LiteLLMModelInfo {
		return { supportsPromptCache: o.supportsPromptCache ?? false, ...o }
	},
}

export interface OcaModelInfo {
	maxTokens?: number
	contextWindow?: number
	supportsImages?: boolean
	supportsPromptCache: boolean
	inputPrice?: number
	outputPrice?: number
	thinkingConfig?: ThinkingConfig
	cacheWritesPrice?: number
	cacheReadsPrice?: number
	description?: string
	temperature?: number
	surveyContent?: string
	surveyId?: string
	banner?: string
	modelName: string
	apiFormat?: ApiFormat
	supportsReasoning?: boolean
	reasoningEffortOptions?: string[]
	supportsTools?: boolean
}
export const OcaModelInfo = {
	create(o: Partial<OcaModelInfo> = {}): OcaModelInfo {
		return { supportsPromptCache: o.supportsPromptCache ?? false, modelName: o.modelName ?? "", ...o }
	},
}

export interface OcaCompatibleModelInfo {
	models: Record<string, OcaModelInfo>
	error?: string
}
export const OcaCompatibleModelInfo = {
	create(o: Partial<OcaCompatibleModelInfo> = {}): OcaCompatibleModelInfo {
		return { models: o.models ?? {} }
	},
}

export interface VsCodeLmModelsArray {
	models: LanguageModelChatSelector[]
}
export const VsCodeLmModelsArray = {
	create(o: Partial<VsCodeLmModelsArray> = {}): VsCodeLmModelsArray {
		return { models: o.models ?? [] }
	},
}

export interface OpenAiModelsRequest {
	metadata: Metadata
	baseUrl: string
	apiKey: string
}
export const OpenAiModelsRequest = {
	create(o: Partial<OpenAiModelsRequest> = {}): OpenAiModelsRequest {
		return { metadata: o.metadata ?? Metadata.create(), baseUrl: o.baseUrl ?? "", apiKey: o.apiKey ?? "" }
	},
}

export interface SapAiCoreModelsRequest {
	metadata: Metadata
	clientId: string
	clientSecret: string
	baseUrl: string
	tokenUrl: string
	resourceGroup: string
}
export const SapAiCoreModelsRequest = {
	create(o: Partial<SapAiCoreModelsRequest> = {}): SapAiCoreModelsRequest {
		return { metadata: o.metadata ?? Metadata.create(), clientId: o.clientId ?? "", clientSecret: o.clientSecret ?? "", baseUrl: o.baseUrl ?? "", tokenUrl: o.tokenUrl ?? "", resourceGroup: o.resourceGroup ?? "" }
	},
}

export interface SapAiCoreModelDeployment {
	modelName: string
	deploymentId: string
}
export const SapAiCoreModelDeployment = {
	create(o: Partial<SapAiCoreModelDeployment> = {}): SapAiCoreModelDeployment {
		return { modelName: o.modelName ?? "", deploymentId: o.deploymentId ?? "" }
	},
}

export interface SapAiCoreModelsResponse {
	deployments: SapAiCoreModelDeployment[]
	orchestrationAvailable: boolean
}
export const SapAiCoreModelsResponse = {
	create(o: Partial<SapAiCoreModelsResponse> = {}): SapAiCoreModelsResponse {
		return { deployments: o.deployments ?? [], orchestrationAvailable: o.orchestrationAvailable ?? false }
	},
}

export interface ModelsApiSecrets {
	apiKey?: string
	diracApiKey?: string
	liteLlmApiKey?: string
	openRouterApiKey?: string
	awsAccessKey?: string
	awsSecretKey?: string
	awsSessionToken?: string
	awsBedrockApiKey?: string
	openAiApiKey?: string
	geminiApiKey?: string
	openAiNativeApiKey?: string
	deepSeekApiKey?: string
	requestyApiKey?: string
	togetherApiKey?: string
	fireworksApiKey?: string
	qwenApiKey?: string
	doubaoApiKey?: string
	mistralApiKey?: string
	nebiusApiKey?: string
	asksageApiKey?: string
	xaiApiKey?: string
	sambanovaApiKey?: string
	cerebrasApiKey?: string
	sapAiCoreClientId?: string
	sapAiCoreClientSecret?: string
	moonshotApiKey?: string
	diracAccountId?: string
	groqApiKey?: string
	huggingFaceApiKey?: string
	huaweiCloudMaasApiKey?: string
	basetenApiKey?: string
	zaiApiKey?: string
	vercelAiGatewayApiKey?: string
	difyApiKey?: string
	minimaxApiKey?: string
	aihubmixApiKey?: string
}
export const ModelsApiSecrets = {
	create(o: Partial<ModelsApiSecrets> = {}): ModelsApiSecrets {
		return { ...o }
	},
}

export interface ModelsApiOptions {
	ulid?: string
	liteLlmBaseUrl?: string
	liteLlmUsePromptCache?: boolean
	openAiHeaders?: Record<string, string>
	anthropicBaseUrl?: string
	openRouterProviderSorting?: string
	awsRegion?: string
	awsUseCrossRegionInference?: boolean
	awsBedrockUsePromptCache?: boolean
	awsUseProfile?: boolean
	awsProfile?: string
	awsBedrockEndpoint?: string
	claudeCodePath?: string
	vertexProjectId?: string
	vertexRegion?: string
	openAiBaseUrl?: string
	lmStudioBaseUrl?: string
	geminiBaseUrl?: string
	requestyBaseUrl?: string
	fireworksModelMaxCompletionTokens?: number
	fireworksModelMaxTokens?: number
	azureApiVersion?: string
	qwenApiLine?: string
	asksageApiUrl?: string
	requestTimeoutMs?: number
	sapAiResourceGroup?: string
	sapAiCoreTokenUrl?: string
	sapAiCoreBaseUrl?: string
	sapAiCoreUseOrchestrationMode?: boolean
	moonshotApiLine?: string
	awsAuthentication?: string
	zaiApiLine?: string
	lmStudioMaxTokens?: string
	qwenCodeOauthPath?: string
	difyBaseUrl?: string
	awsUseGlobalInference?: boolean
	minimaxApiLine?: string
	aihubmixBaseUrl?: string
	aihubmixAppCode?: string
	azureIdentity?: boolean
	planModeApiProvider?: ApiProvider
	planModeApiModelId?: string
	planModeThinkingBudgetTokens?: number
	planModeReasoningEffort?: string
	planModeVsCodeLmModelSelector?: LanguageModelChatSelector
	planModeAwsBedrockCustomSelected?: boolean
	planModeAwsBedrockCustomModelBaseId?: string
	planModeOpenRouterModelId?: string
	planModeOpenRouterModelInfo?: OpenRouterModelInfo
	planModeOpenAiModelId?: string
	planModeOpenAiModelInfo?: OpenAiCompatibleModelInfo
	planModeLmStudioModelId?: string
	planModeLiteLlmModelId?: string
	planModeLiteLlmModelInfo?: LiteLLMModelInfo
	planModeRequestyModelId?: string
	planModeRequestyModelInfo?: OpenRouterModelInfo
	planModeTogetherModelId?: string
	planModeFireworksModelId?: string
	planModeSapAiCoreModelId?: string
	planModeSapAiCoreDeploymentId?: string
	planModeGroqModelId?: string
	planModeGroqModelInfo?: OpenRouterModelInfo
	planModeHuggingFaceModelId?: string
	planModeHuggingFaceModelInfo?: OpenRouterModelInfo
	planModeHuaweiCloudMaasModelId?: string
	planModeHuaweiCloudMaasModelInfo?: OpenRouterModelInfo
	planModeBasetenModelId?: string
	planModeBasetenModelInfo?: OpenRouterModelInfo
	planModeVercelAiGatewayModelId?: string
	planModeVercelAiGatewayModelInfo?: OpenRouterModelInfo
	planModeAihubmixModelId?: string
	planModeAihubmixModelInfo?: OpenAiCompatibleModelInfo
	planModeDiracModelId?: string
	planModeDiracModelInfo?: OpenRouterModelInfo
	actModeApiProvider?: ApiProvider
	actModeApiModelId?: string
	actModeThinkingBudgetTokens?: number
	actModeReasoningEffort?: string
	actModeVsCodeLmModelSelector?: LanguageModelChatSelector
	actModeAwsBedrockCustomSelected?: boolean
	actModeAwsBedrockCustomModelBaseId?: string
	actModeOpenRouterModelId?: string
	actModeOpenRouterModelInfo?: OpenRouterModelInfo
	actModeOpenAiModelId?: string
	actModeOpenAiModelInfo?: OpenAiCompatibleModelInfo
	actModeLmStudioModelId?: string
	actModeLiteLlmModelId?: string
	actModeLiteLlmModelInfo?: LiteLLMModelInfo
	actModeRequestyModelId?: string
	actModeRequestyModelInfo?: OpenRouterModelInfo
	actModeTogetherModelId?: string
	actModeFireworksModelId?: string
	actModeSapAiCoreModelId?: string
	actModeSapAiCoreDeploymentId?: string
	actModeGroqModelId?: string
	actModeGroqModelInfo?: OpenRouterModelInfo
	actModeHuggingFaceModelId?: string
	actModeHuggingFaceModelInfo?: OpenRouterModelInfo
	actModeHuaweiCloudMaasModelId?: string
	actModeHuaweiCloudMaasModelInfo?: OpenRouterModelInfo
	actModeBasetenModelId?: string
	actModeBasetenModelInfo?: OpenRouterModelInfo
	actModeVercelAiGatewayModelId?: string
	actModeVercelAiGatewayModelInfo?: OpenRouterModelInfo
	actModeAihubmixModelId?: string
	actModeAihubmixModelInfo?: OpenAiCompatibleModelInfo
	actModeDiracModelId?: string
	actModeDiracModelInfo?: OpenRouterModelInfo
}
export const ModelsApiOptions = {
	create(o: Partial<ModelsApiOptions> = {}): ModelsApiOptions {
		return { ...o }
	},
}

export interface ApiConfiguration {
	options?: ModelsApiOptions
	secrets?: ModelsApiSecrets
}
export const ApiConfiguration = {
	create(o: Partial<ApiConfiguration> = {}): ApiConfiguration {
		return { ...o }
	},
}

export interface UpdateApiConfigurationRequest {
	metadata: Metadata
	apiConfiguration: ModelsApiConfiguration
}
export const UpdateApiConfigurationRequest = {
	create(o: Partial<UpdateApiConfigurationRequest> = {}): UpdateApiConfigurationRequest {
		return { metadata: o.metadata ?? Metadata.create(), apiConfiguration: o.apiConfiguration ?? ModelsApiConfiguration.create() }
	},
}

export interface UpdateApiConfigurationRequestNew {
	metadata: Metadata
	updates?: ApiConfiguration
	updateMask?: string[]
}
export const UpdateApiConfigurationRequestNew = {
	create(o: Partial<UpdateApiConfigurationRequestNew> = {}): UpdateApiConfigurationRequestNew {
		return { metadata: o.metadata ?? Metadata.create(), ...o }
	},
}

export interface UpdateApiConfigurationPartialRequest {
	metadata: Metadata
	apiConfiguration: ModelsApiConfiguration
	updateMask?: any
}
export const UpdateApiConfigurationPartialRequest = {
	create(o: Partial<UpdateApiConfigurationPartialRequest> = {}): UpdateApiConfigurationPartialRequest {
		return { metadata: o.metadata ?? Metadata.create(), apiConfiguration: o.apiConfiguration ?? ModelsApiConfiguration.create() }
	},
}

export interface ModelsApiConfiguration {
	apiKey?: string
	diracApiKey?: string
	ulid?: string
	liteLlmBaseUrl?: string
	liteLlmApiKey?: string
	liteLlmUsePromptCache?: boolean
	openAiHeaders?: Record<string, string>
	anthropicBaseUrl?: string
	openRouterApiKey?: string
	openRouterProviderSorting?: string
	awsAccessKey?: string
	awsSecretKey?: string
	awsSessionToken?: string
	awsRegion?: string
	awsUseCrossRegionInference?: boolean
	awsBedrockUsePromptCache?: boolean
	awsUseProfile?: boolean
	awsProfile?: string
	awsBedrockEndpoint?: string
	claudeCodePath?: string
	vertexProjectId?: string
	vertexRegion?: string
	openAiBaseUrl?: string
	openAiApiKey?: string
	lmStudioBaseUrl?: string
	geminiApiKey?: string
	geminiBaseUrl?: string
	openAiNativeApiKey?: string
	deepSeekApiKey?: string
	requestyApiKey?: string
	requestyBaseUrl?: string
	togetherApiKey?: string
	fireworksApiKey?: string
	fireworksModelMaxCompletionTokens?: number
	fireworksModelMaxTokens?: number
	qwenApiKey?: string
	doubaoApiKey?: string
	mistralApiKey?: string
	azureApiVersion?: string
	qwenApiLine?: string
	nebiusApiKey?: string
	asksageApiUrl?: string
	asksageApiKey?: string
	xaiApiKey?: string
	sambanovaApiKey?: string
	cerebrasApiKey?: string
	requestTimeoutMs?: number
	sapAiCoreClientId?: string
	sapAiCoreClientSecret?: string
	sapAiResourceGroup?: string
	sapAiCoreTokenUrl?: string
	sapAiCoreBaseUrl?: string
	sapAiCoreUseOrchestrationMode?: boolean
	moonshotApiKey?: string
	moonshotApiLine?: string
	awsAuthentication?: string
	awsBedrockApiKey?: string
	groqApiKey?: string
	huggingFaceApiKey?: string
	huaweiCloudMaasApiKey?: string
	basetenApiKey?: string
	zaiApiKey?: string
	zaiApiLine?: string
	lmStudioMaxTokens?: string
	vercelAiGatewayApiKey?: string
	qwenCodeOAuthPath?: string
	difyApiKey?: string
	difyBaseUrl?: string
	awsUseGlobalInference?: boolean
	minimaxApiKey?: string
	minimaxApiLine?: string
	hicapModelId?: string
	hicapApiKey?: string
	aihubmixApiKey?: string
	aihubmixBaseUrl?: string
	aihubmixAppCode?: string
	nousResearchApiKey?: string
	azureIdentity?: boolean
	wandbApiKey?: string
	planModeApiProvider?: ApiProvider
	planModeApiModelId?: string
	planModeThinkingBudgetTokens?: number
	planModeReasoningEffort?: string
	planModeVsCodeLmModelSelector?: LanguageModelChatSelector
	planModeAwsBedrockCustomSelected?: boolean
	planModeAwsBedrockCustomModelBaseId?: string
	planModeOpenRouterModelId?: string
	planModeOpenRouterModelInfo?: OpenRouterModelInfo
	planModeOpenAiModelId?: string
	planModeOpenAiModelInfo?: OpenAiCompatibleModelInfo
	planModeLmStudioModelId?: string
	planModeLiteLlmModelId?: string
	planModeLiteLlmModelInfo?: LiteLLMModelInfo
	planModeRequestyModelId?: string
	planModeRequestyModelInfo?: OpenRouterModelInfo
	planModeTogetherModelId?: string
	planModeFireworksModelId?: string
	planModeSapAiCoreModelId?: string
	planModeSapAiCoreDeploymentId?: string
	planModeGroqModelId?: string
	planModeGroqModelInfo?: OpenRouterModelInfo
	planModeHuggingFaceModelId?: string
	planModeHuggingFaceModelInfo?: OpenRouterModelInfo
	planModeHuaweiCloudMaasModelId?: string
	planModeHuaweiCloudMaasModelInfo?: OpenRouterModelInfo
	planModeBasetenModelId?: string
	planModeBasetenModelInfo?: OpenRouterModelInfo
	planModeVercelAiGatewayModelId?: string
	planModeVercelAiGatewayModelInfo?: OpenRouterModelInfo
	planModeHicapModelId?: string
	planModeHicapModelInfo?: OpenRouterModelInfo
	planModeAihubmixModelId?: string
	planModeAihubmixModelInfo?: OpenAiCompatibleModelInfo
	planModeNousResearchModelId?: string
	geminiPlanModeThinkingLevel?: string
	planModeDiracModelId?: string
	planModeDiracModelInfo?: OpenRouterModelInfo
	actModeApiProvider?: ApiProvider
	actModeApiModelId?: string
	actModeThinkingBudgetTokens?: number
	actModeReasoningEffort?: string
	actModeVsCodeLmModelSelector?: LanguageModelChatSelector
	actModeAwsBedrockCustomSelected?: boolean
	actModeAwsBedrockCustomModelBaseId?: string
	actModeOpenRouterModelId?: string
	actModeOpenRouterModelInfo?: OpenRouterModelInfo
	actModeOpenAiModelId?: string
	actModeOpenAiModelInfo?: OpenAiCompatibleModelInfo
	actModeLmStudioModelId?: string
	actModeLiteLlmModelId?: string
	actModeLiteLlmModelInfo?: LiteLLMModelInfo
	actModeRequestyModelId?: string
	actModeRequestyModelInfo?: OpenRouterModelInfo
	actModeTogetherModelId?: string
	actModeFireworksModelId?: string
	actModeSapAiCoreModelId?: string
	actModeSapAiCoreDeploymentId?: string
	actModeGroqModelId?: string
	actModeGroqModelInfo?: OpenRouterModelInfo
	actModeHuggingFaceModelId?: string
	actModeHuggingFaceModelInfo?: OpenRouterModelInfo
	actModeHuaweiCloudMaasModelId?: string
	actModeHuaweiCloudMaasModelInfo?: OpenRouterModelInfo
	actModeBasetenModelId?: string
	actModeBasetenModelInfo?: OpenRouterModelInfo
	actModeVercelAiGatewayModelId?: string
	actModeVercelAiGatewayModelInfo?: OpenRouterModelInfo
	actModeHicapModelId?: string
	actModeHicapModelInfo?: OpenRouterModelInfo
	actModeAihubmixModelId?: string
	actModeAihubmixModelInfo?: OpenAiCompatibleModelInfo
	actModeNousResearchModelId?: string
	geminiActModeThinkingLevel?: string
	actModeDiracModelId?: string
	actModeDiracModelInfo?: OpenRouterModelInfo
}
export const ModelsApiConfiguration = {
	create(o: Partial<ModelsApiConfiguration> = {}): ModelsApiConfiguration {
		return { ...o }
	},
}
