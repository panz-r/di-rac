import type { ApiConfiguration, ApiProvider } from "@shared/api"
import { getProviderDefaultModelId } from "@shared/providers/provider-registry"

export type ModelRole = "act" | "plan" | "observe"

export type Mode = "act" | "plan"

// --- Role key map: maps (role, suffix) → state key name ---

const ROLE_KEY_MAP: Record<string, string> = {
	// act
	"act.provider": "actModeApiProvider",
	"act.apiModelId": "actModeApiModelId",
	"act.thinkingBudgetTokens": "actModeThinkingBudgetTokens",
	"act.reasoningEffort": "actModeReasoningEffort",
	"act.verbosity": "actModeVerbosity",
	"act.geminiThinkingLevel": "geminiActModeThinkingLevel",
	"act.vsCodeLmModelSelector": "actModeVsCodeLmModelSelector",
	"act.openRouterModelId": "actModeOpenRouterModelId",
	"act.openRouterModelInfo": "actModeOpenRouterModelInfo",
	"act.diracModelId": "actModeDiracModelId",
	"act.diracModelInfo": "actModeDiracModelInfo",
	"act.openAiModelId": "actModeOpenAiModelId",
	"act.openAiModelInfo": "actModeOpenAiModelInfo",
	"act.lmStudioModelId": "actModeLmStudioModelId",
	"act.liteLlmModelId": "actModeLiteLlmModelId",
	"act.liteLlmModelInfo": "actModeLiteLlmModelInfo",
	"act.codingPlanZAiModelId": "actModeCodingPlanZAiModelId",
	"act.codingPlanZAiModelInfo": "actModeCodingPlanZAiModelInfo",
	"act.requestyModelId": "actModeRequestyModelId",
	"act.requestyModelInfo": "actModeRequestyModelInfo",
	"act.togetherModelId": "actModeTogetherModelId",
	"act.fireworksModelId": "actModeFireworksModelId",
	"act.groqModelId": "actModeGroqModelId",
	"act.groqModelInfo": "actModeGroqModelInfo",
	"act.huggingFaceModelId": "actModeHuggingFaceModelId",
	"act.huggingFaceModelInfo": "actModeHuggingFaceModelInfo",
	"act.huaweiCloudMaasModelId": "actModeHuaweiCloudMaasModelId",
	"act.huaweiCloudMaasModelInfo": "actModeHuaweiCloudMaasModelInfo",
	"act.hicapModelId": "actModeHicapModelId",
	"act.hicapModelInfo": "actModeHicapModelInfo",
	"act.nousResearchModelId": "actModeNousResearchModelId",
	"act.nvidiaNimModelId": "actModeNvidiaNimModelId",
	"act.vercelAiGatewayModelId": "actModeVercelAiGatewayModelId",
	"act.vercelAiGatewayModelInfo": "actModeVercelAiGatewayModelInfo",
	// plan
	"plan.provider": "planModeApiProvider",
	"plan.apiModelId": "planModeApiModelId",
	"plan.thinkingBudgetTokens": "planModeThinkingBudgetTokens",
	"plan.reasoningEffort": "planModeReasoningEffort",
	"plan.verbosity": "planModeVerbosity",
	"plan.geminiThinkingLevel": "geminiPlanModeThinkingLevel",
	"plan.vsCodeLmModelSelector": "planModeVsCodeLmModelSelector",
	"plan.openRouterModelId": "planModeOpenRouterModelId",
	"plan.openRouterModelInfo": "planModeOpenRouterModelInfo",
	"plan.diracModelId": "planModeDiracModelId",
	"plan.diracModelInfo": "planModeDiracModelInfo",
	"plan.openAiModelId": "planModeOpenAiModelId",
	"plan.openAiModelInfo": "planModeOpenAiModelInfo",
	"plan.lmStudioModelId": "planModeLmStudioModelId",
	"plan.liteLlmModelId": "planModeLiteLlmModelId",
	"plan.liteLlmModelInfo": "planModeLiteLlmModelInfo",
	"plan.codingPlanZAiModelId": "planModeCodingPlanZAiModelId",
	"plan.codingPlanZAiModelInfo": "planModeCodingPlanZAiModelInfo",
	"plan.requestyModelId": "planModeRequestyModelId",
	"plan.requestyModelInfo": "planModeRequestyModelInfo",
	"plan.togetherModelId": "planModeTogetherModelId",
	"plan.fireworksModelId": "planModeFireworksModelId",
	"plan.groqModelId": "planModeGroqModelId",
	"plan.groqModelInfo": "planModeGroqModelInfo",
	"plan.huggingFaceModelId": "planModeHuggingFaceModelId",
	"plan.huggingFaceModelInfo": "planModeHuggingFaceModelInfo",
	"plan.huaweiCloudMaasModelId": "planModeHuaweiCloudMaasModelId",
	"plan.huaweiCloudMaasModelInfo": "planModeHuaweiCloudMaasModelInfo",
	"plan.hicapModelId": "planModeHicapModelId",
	"plan.hicapModelInfo": "planModeHicapModelInfo",
	"plan.nousResearchModelId": "planModeNousResearchModelId",
	"plan.nvidiaNimModelId": "planModeNvidiaNimModelId",
	"plan.vercelAiGatewayModelId": "planModeVercelAiGatewayModelId",
	"plan.vercelAiGatewayModelInfo": "planModeVercelAiGatewayModelInfo",
	// observe (minimal own keys, falls back to act for everything else)
	"observe.provider": "observerProvider",
	"observe.apiModelId": "observerModelId",
}

/**
 * Resolves a role-specific state key from a generic suffix.
 * For observe (and future roles), unknown suffixes fall back to act's keys.
 */
export function getRoleStateKey(role: ModelRole, suffix: string): string {
	const key = `${role}.${suffix}`
	if (key in ROLE_KEY_MAP) return ROLE_KEY_MAP[key]
	if (role !== "act") return getRoleStateKey("act", suffix)
	return suffix
}

/**
 * Read a role-scoped value from an ApiConfiguration object.
 */
export function getRoleConfigValue(config: ApiConfiguration, role: ModelRole, suffix: string): unknown {
	return (config as Record<string, unknown>)[getRoleStateKey(role, suffix)]
}

// --- Role UI descriptors for settings panel ---

export interface RoleDescriptor {
	role: ModelRole
	label: string
	providerKey: string
	modelKey: string
	enabledKey?: string
	providerInheritsFromAct?: boolean
}

export const ROLE_DESCRIPTORS: RoleDescriptor[] = [
	{
		role: "act",
		label: "Act",
		providerKey: "actModeApiProvider",
		modelKey: "actModeApiModelId",
	},
	{
		role: "plan",
		label: "Plan",
		providerKey: "planModeApiProvider",
		modelKey: "planModeApiModelId",
		providerInheritsFromAct: true,
	},
	{
		role: "observe",
		label: "Observer",
		providerKey: "observerProvider",
		modelKey: "observerModelId",
		enabledKey: "observerEnabled",
		providerInheritsFromAct: true,
	},
]

export function getRoleDescriptor(role: ModelRole): RoleDescriptor {
	return ROLE_DESCRIPTORS.find((d) => d.role === role)!
}
