// Replaces proto-generated dirac/state types with plain TypeScript.

import { Metadata } from "./common"

export { Metadata } from "./common"
export type { Empty, BooleanRequest, EmptyRequest, Int64Request, StringRequest, Boolean, StringArrayRequest } from "./common"

export enum PlanActMode {
	PLAN = 0,
	ACT = 1,
}

export enum OpenaiReasoningEffort {
	LOW = 0,
	MEDIUM = 1,
	HIGH = 2,
}

export enum TelemetrySettingEnum {
	UNSET = 0,
	ENABLED = 1,
	DISABLED = 2,
}

export interface AutoApprovalActions {
	readFiles?: boolean
	readFilesExternally?: boolean
	editFiles?: boolean
	editFilesExternally?: boolean
	executeSafeCommands?: boolean
	executeAllCommands?: boolean
	useBrowser?: boolean
}
export const AutoApprovalActions = {
	create(o: Partial<AutoApprovalActions> = {}): AutoApprovalActions {
		return { ...o }
	},
}

export interface AutoApprovalSettings {
	version: number
	actions?: AutoApprovalActions
	enableNotifications?: boolean
}
export const AutoApprovalSettings = {
	create(o: Partial<AutoApprovalSettings> = {}): AutoApprovalSettings {
		return { version: o.version ?? 0, ...o }
	},
}

export interface Secrets {
	apiKey?: string
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
	liteLlmApiKey?: string
	asksageApiKey?: string
	xaiApiKey?: string
	moonshotApiKey?: string
	zaiApiKey?: string
	huggingFaceApiKey?: string
	nebiusApiKey?: string
	sambanovaApiKey?: string
	cerebrasApiKey?: string
	sapAiCoreClientId?: string
	sapAiCoreClientSecret?: string
	groqApiKey?: string
	huaweiCloudMaasApiKey?: string
	basetenApiKey?: string
	vercelAiGatewayApiKey?: string
	difyApiKey?: string
	minimaxApiKey?: string
	hicapApiKey?: string
	aihubmixApiKey?: string
	nousResearchApiKey?: string
	wandbApiKey?: string
	diracApiKey?: string
	authNonce?: string
	openaiCodexOauthCredentials?: string
	openAiCompatibleCustomApiKey?: string
}
export const Secrets = {
	create(o: Partial<Secrets> = {}): Secrets {
		return { ...o }
	},
}

export interface Viewport {
	width?: number
	height?: number
}
export const Viewport = {
	create(o: Partial<Viewport> = {}): Viewport {
		return { ...o }
	},
}

export interface BrowserSettings {
	viewport?: Viewport
	remoteBrowserHost?: string
	remoteBrowserEnabled?: boolean
	chromeExecutablePath?: string
	disableToolUse?: boolean
	customArgs?: string
}
export const BrowserSettings = {
	create(o: Partial<BrowserSettings> = {}): BrowserSettings {
		return { ...o }
	},
}

export interface Settings {
	liteLlmBaseUrl?: string
	liteLlmUsePromptCache?: boolean
	anthropicBaseUrl?: string
	openRouterProviderSorting?: string
	awsRegion?: string
	awsUseCrossRegionInference?: boolean
	awsUseGlobalInference?: boolean
	awsBedrockUsePromptCache?: boolean
	awsAuthentication?: string
	awsUseProfile?: boolean
	awsProfile?: string
	awsBedrockEndpoint?: string
	claudeCodePath?: string
	vertexProjectId?: string
	vertexRegion?: string
	openAiBaseUrl?: string
	lmStudioBaseUrl?: string
	lmStudioMaxTokens?: string
	geminiBaseUrl?: string
	requestyBaseUrl?: string
	fireworksModelMaxCompletionTokens?: number
	fireworksModelMaxTokens?: number
	qwenCodeOauthPath?: string
	azureApiVersion?: string
	qwenApiLine?: string
	moonshotApiLine?: string
	asksageApiUrl?: string
	requestTimeoutMs?: number
	sapAiResourceGroup?: string
	sapAiCoreTokenUrl?: string
	sapAiCoreBaseUrl?: string
	sapAiCoreUseOrchestrationMode?: boolean
	difyBaseUrl?: string
	zaiApiLine?: string
	aihubmixBaseUrl?: string
	aihubmixAppCode?: string
	planModeApiModelId?: string
	planModeThinkingBudgetTokens?: number
	geminiPlanModeThinkingLevel?: string
	planModeReasoningEffort?: string
	planModeVerbosity?: string
	planModeVsCodeLmModelSelector?: any
	planModeAwsBedrockCustomSelected?: boolean
	planModeAwsBedrockCustomModelBaseId?: string
	planModeOpenRouterModelId?: string
	planModeOpenRouterModelInfo?: any
	planModeOpenAiModelId?: string
	planModeOpenAiModelInfo?: any
	planModeLmStudioModelId?: string
	planModeLiteLlmModelId?: string
	planModeLiteLlmModelInfo?: any
	planModeRequestyModelId?: string
	planModeRequestyModelInfo?: any
	planModeTogetherModelId?: string
	planModeFireworksModelId?: string
	planModeSapAiCoreModelId?: string
	planModeSapAiCoreDeploymentId?: string
	planModeGroqModelId?: string
	planModeGroqModelInfo?: any
	planModeBasetenModelId?: string
	planModeBasetenModelInfo?: any
	planModeHuggingFaceModelId?: string
	planModeHuggingFaceModelInfo?: any
	planModeHuaweiCloudMaasModelId?: string
	planModeHuaweiCloudMaasModelInfo?: any
	planModeAihubmixModelId?: string
	planModeAihubmixModelInfo?: any
	planModeHicapModelId?: string
	planModeHicapModelInfo?: any
	planModeNousResearchModelId?: string
	planModeVercelAiGatewayModelId?: string
	planModeVercelAiGatewayModelInfo?: any
	actModeApiModelId?: string
	actModeThinkingBudgetTokens?: number
	geminiActModeThinkingLevel?: string
	actModeReasoningEffort?: string
	actModeVerbosity?: string
	actModeVsCodeLmModelSelector?: any
	actModeAwsBedrockCustomSelected?: boolean
	actModeAwsBedrockCustomModelBaseId?: string
	actModeOpenRouterModelId?: string
	actModeOpenRouterModelInfo?: any
	actModeOpenAiModelId?: string
	actModeOpenAiModelInfo?: any
	actModeLmStudioModelId?: string
	actModeLiteLlmModelId?: string
	actModeLiteLlmModelInfo?: any
	actModeRequestyModelId?: string
	actModeRequestyModelInfo?: any
	actModeTogetherModelId?: string
	actModeFireworksModelId?: string
	actModeSapAiCoreModelId?: string
	actModeSapAiCoreDeploymentId?: string
	actModeGroqModelId?: string
	actModeGroqModelInfo?: any
	actModeBasetenModelId?: string
	actModeBasetenModelInfo?: any
	actModeHuggingFaceModelId?: string
	actModeHuggingFaceModelInfo?: any
	actModeHuaweiCloudMaasModelId?: string
	actModeHuaweiCloudMaasModelInfo?: any
	actModeAihubmixModelId?: string
	actModeAihubmixModelInfo?: any
	actModeHicapModelId?: string
	actModeHicapModelInfo?: any
	actModeNousResearchModelId?: string
	actModeVercelAiGatewayModelId?: string
	actModeVercelAiGatewayModelInfo?: any
	planModeApiProvider?: number
	actModeApiProvider?: number
	hicapModelId?: string
	lmStudioModelId?: string
	autoApprovalSettings?: AutoApprovalSettings
	globalDiracRulesToggles?: string
	globalWorkflowToggles?: string
	globalSkillsToggles?: string
	browserSettings?: BrowserSettings
	telemetrySetting?: string
	planActSeparateModelsSetting?: boolean
	shellIntegrationTimeout?: number
	defaultTerminalProfile?: string
	terminalOutputLineLimit?: number
	maxConsecutiveMistakes?: number
	strictPlanModeEnabled?: boolean
	yoloModeToggled?: boolean
	useAutoCondense?: boolean
	diracWebToolsEnabled?: boolean
	preferredLanguage?: string
	mode?: PlanActMode
	customPrompt?: string
	hooksEnabled?: boolean
	subagentsEnabled?: boolean
	enableParallelToolCalling?: boolean
	backgroundEditEnabled?: boolean
	openTelemetryEnabled?: boolean
	openTelemetryMetricsExporter?: string
	openTelemetryLogsExporter?: string
	openTelemetryOtlpProtocol?: string
	openTelemetryOtlpEndpoint?: string
	openTelemetryOtlpMetricsProtocol?: string
	openTelemetryOtlpMetricsEndpoint?: string
	openTelemetryOtlpLogsProtocol?: string
	openTelemetryOtlpLogsEndpoint?: string
	openTelemetryMetricExportInterval?: number
	openTelemetryOtlpInsecure?: boolean
	openTelemetryLogBatchSize?: number
	openTelemetryLogBatchTimeout?: number
	openTelemetryLogMaxQueueSize?: number
	worktreesEnabled?: boolean
	autoApproveAllToggled?: boolean
	doubleCheckCompletionEnabled?: boolean
	openAiHeaders?: Record<string, string>
	planModeDiracModelId?: string
	planModeDiracModelInfo?: any
	actModeDiracModelId?: string
	actModeDiracModelInfo?: any
	writePromptMetadataEnabled?: boolean
	writePromptMetadataDirectory?: string
	minimaxApiLine?: string
	optOutOfRemoteConfig?: boolean
	geminiSearchEnabled?: boolean
	rewritePaths?: boolean
	bashToolEnabled?: boolean
	bashAutoApproveAll?: boolean
}
export const Settings = {
	create(o: Partial<Settings> = {}): Settings {
		return { ...o }
	},
}

export interface State {
	stateJson: string
}
export const State = {
	create(o: Partial<State> = {}): State {
		return { stateJson: o.stateJson ?? "" }
	},
}

export interface TerminalProfile {
	id: string
	name: string
	path?: string
	description?: string
}
export const TerminalProfile = {
	create(o: Partial<TerminalProfile> = {}): TerminalProfile {
		return { id: o.id ?? "", name: o.name ?? "", ...o }
	},
}

export interface TerminalProfiles {
	profiles: TerminalProfile[]
}
export const TerminalProfiles = {
	create(o: Partial<TerminalProfiles> = {}): TerminalProfiles {
		return { profiles: o.profiles ?? [] }
	},
}

export interface TerminalProfileUpdateResponse {
	closedCount: number
	busyTerminalsCount: number
	hasBusyTerminals: boolean
}
export const TerminalProfileUpdateResponse = {
	create(o: Partial<TerminalProfileUpdateResponse> = {}): TerminalProfileUpdateResponse {
		return { closedCount: o.closedCount ?? 0, busyTerminalsCount: o.busyTerminalsCount ?? 0, hasBusyTerminals: o.hasBusyTerminals ?? false }
	},
}

export interface TogglePlanActModeRequest {
	metadata: Metadata
	mode: PlanActMode
	chatContent?: ChatContent
}
export const TogglePlanActModeRequest = {
	create(o: Partial<TogglePlanActModeRequest> = {}): TogglePlanActModeRequest {
		return { metadata: o.metadata ?? Metadata.create(), mode: o.mode ?? PlanActMode.PLAN, ...o }
	},
}

export interface ChatContent {
	message?: string
	images: string[]
	files: string[]
}
export const ChatContent = {
	create(o: Partial<ChatContent> = {}): ChatContent {
		return { images: o.images ?? [], files: o.files ?? [], ...o }
	},
}

export interface ResetStateRequest {
	metadata: Metadata
	global?: boolean
}
export const ResetStateRequest = {
	create(o: Partial<ResetStateRequest> = {}): ResetStateRequest {
		return { metadata: o.metadata ?? Metadata.create(), ...o }
	},
}

export interface AutoApprovalSettingsRequest {
	metadata: Metadata
	version: number
	actions: AutoApprovalActions
	enableNotifications: boolean
}
export const AutoApprovalSettingsRequest = {
	create(o: Partial<AutoApprovalSettingsRequest> = {}): AutoApprovalSettingsRequest {
		return { metadata: o.metadata ?? Metadata.create(), version: o.version ?? 0, actions: o.actions ?? AutoApprovalActions.create(), enableNotifications: o.enableNotifications ?? false }
	},
}

export interface TelemetrySettingRequest {
	metadata: Metadata
	setting: TelemetrySettingEnum
}
export const TelemetrySettingRequest = {
	create(o: Partial<TelemetrySettingRequest> = {}): TelemetrySettingRequest {
		return { metadata: o.metadata ?? Metadata.create(), setting: o.setting ?? TelemetrySettingEnum.UNSET }
	},
}

export interface BrowserSettingsUpdate {
	viewport?: Viewport
	remoteBrowserHost?: string
	remoteBrowserEnabled?: boolean
	chromeExecutablePath?: string
	disableToolUse?: boolean
	customArgs?: string
}
export const BrowserSettingsUpdate = {
	create(o: Partial<BrowserSettingsUpdate> = {}): BrowserSettingsUpdate {
		return { ...o }
	},
}

export interface UpdateSettingsRequestCli {
	metadata: Metadata
	settings?: Settings
	secrets?: Secrets
	environment?: string
}
export const UpdateSettingsRequestCli = {
	create(o: Partial<UpdateSettingsRequestCli> = {}): UpdateSettingsRequestCli {
		return { metadata: o.metadata ?? Metadata.create(), ...o }
	},
}

export interface UpdateTaskSettingsRequest {
	metadata: Metadata
	settings?: Settings
	taskId?: string
}
export const UpdateTaskSettingsRequest = {
	create(o: Partial<UpdateTaskSettingsRequest> = {}): UpdateTaskSettingsRequest {
		return { metadata: o.metadata ?? Metadata.create(), ...o }
	},
}

export interface UpdateSettingsRequest {
	metadata: Metadata
	apiConfiguration?: any
	telemetrySetting?: string
	planActSeparateModelsSetting?: boolean
	shellIntegrationTimeout?: number
	terminalReuseEnabled?: boolean
	terminalOutputLineLimit?: number
	mode?: PlanActMode
	preferredLanguage?: string
	strictPlanModeEnabled?: boolean
	useAutoCondense?: boolean
	customPrompt?: string
	browserSettings?: BrowserSettingsUpdate
	defaultTerminalProfile?: string
	yoloModeToggled?: boolean
	multiRootEnabled?: boolean
	hooksEnabled?: boolean
	vscodeTerminalExecutionMode?: string
	maxConsecutiveMistakes?: number
	subagentsEnabled?: boolean
	subagentTerminalOutputLineLimit?: number
	diracEnv?: string
	nativeToolCallEnabled?: boolean
	onboardingModels?: OnboardingModelGroup
	diracWebToolsEnabled?: boolean
	enableParallelToolCalling?: boolean
	backgroundEditEnabled?: boolean
	worktreesEnabled?: boolean
	doubleCheckCompletionEnabled?: boolean
	writePromptMetadataEnabled?: boolean
	writePromptMetadataDirectory?: string
}
export const UpdateSettingsRequest = {
	create(o: Partial<UpdateSettingsRequest> = {}): UpdateSettingsRequest {
		return { metadata: o.metadata ?? Metadata.create(), ...o }
	},
}

export interface UpdateTerminalConnectionTimeoutRequest {
	timeoutMs?: number
}
export const UpdateTerminalConnectionTimeoutRequest = {
	create(o: Partial<UpdateTerminalConnectionTimeoutRequest> = {}): UpdateTerminalConnectionTimeoutRequest {
		return { ...o }
	},
}

export interface UpdateTerminalConnectionTimeoutResponse {
	timeoutMs?: number
}
export const UpdateTerminalConnectionTimeoutResponse = {
	create(o: Partial<UpdateTerminalConnectionTimeoutResponse> = {}): UpdateTerminalConnectionTimeoutResponse {
		return { ...o }
	},
}

export interface ProcessInfo {
	processId: number
	version?: string
	uptimeMs?: number
}
export const ProcessInfo = {
	create(o: Partial<ProcessInfo> = {}): ProcessInfo {
		return { processId: o.processId ?? 0, ...o }
	},
}

export interface OnboardingProgressRequest {
	step: number
	action?: string
	completed?: boolean
	modelSelected?: string
}
export const OnboardingProgressRequest = {
	create(o: Partial<OnboardingProgressRequest> = {}): OnboardingProgressRequest {
		return { step: o.step ?? 0, ...o }
	},
}

export interface OnboardingModelGroup {
	models: OnboardingModel[]
}
export const OnboardingModelGroup = {
	create(o: Partial<OnboardingModelGroup> = {}): OnboardingModelGroup {
		return { models: o.models ?? [] }
	},
}

export interface OnboardingModel {
	id: string
	name: string
	score: number
	latency: number
	badge: string
	group: string
	info?: any
}
export const OnboardingModel = {
	create(o: Partial<OnboardingModel> = {}): OnboardingModel {
		return { id: o.id ?? "", name: o.name ?? "", score: o.score ?? 0, latency: o.latency ?? 0, badge: o.badge ?? "", group: o.group ?? "" }
	},
}

export interface TrackBannerEventRequest {
	bannerId: string
	eventType: string
}
export const TrackBannerEventRequest = {
	create(o: Partial<TrackBannerEventRequest> = {}): TrackBannerEventRequest {
		return { bannerId: o.bannerId ?? "", eventType: o.eventType ?? "" }
	},
}

export interface TestConnectionResult {
	success: boolean
	message?: string
	error?: string
}
export const TestConnectionResult = {
	create(o: Partial<TestConnectionResult> = {}): TestConnectionResult {
		return { success: o.success ?? false, ...o }
	},
}
