/**
 * Shared utility for applying provider configuration
 * Used by both AuthView (onboarding) and SettingsPanelContent (settings)
 */

import type { ApiProvider } from "@shared/api"
import { getProviderModelIdKey, ProviderToApiKeyMap } from "@shared/storage"
import type { ModelRole } from "@shared/roles"
import { getRoleStateKey } from "@shared/roles"
import { buildApiHandler } from "@/core/api"
import type { Controller } from "@/core/controller"
import { StateManager } from "@/core/storage/StateManager"
import type { BedrockConfig } from "../components/BedrockSetup"
import { getDefaultModelId } from "../components/ModelPicker"

export interface ApplyProviderConfigOptions {
	providerId: string
	role?: ModelRole
	apiKey?: string
	modelId?: string
	baseUrl?: string
	azureApiVersion?: string
	controller?: Controller
}

/**
 * Apply provider configuration to state and rebuild API handler if needed.
 * When role is specified, writes config for that role only.
 * When role is omitted (default: "act"), also syncs to plan if plan has no provider.
 */
export async function applyProviderConfig(options: ApplyProviderConfigOptions): Promise<void> {
	const { providerId, role = "act", apiKey, modelId, baseUrl, azureApiVersion, controller } = options
	const stateManager = StateManager.get()

	const config: Record<string, string> = {}

	// Set provider for the target role
	config[getRoleStateKey(role, "provider")] = providerId

	// For act role, also set plan if plan has no explicit provider
	if (role === "act") {
		const planProvider = stateManager.getGlobalSettingsKey("planModeApiProvider")
		if (!planProvider) {
			config["planModeApiProvider"] = providerId
		}
	}

	// Add model ID (use provided or fall back to default)
	const finalModelId = modelId || getDefaultModelId(providerId)
	if (finalModelId) {
		const modelKey = getProviderModelIdKey(providerId as ApiProvider, role)
		if (modelKey) config[modelKey] = finalModelId

		// For act role, also sync model to plan if plan has no explicit provider
		if (role === "act") {
			const planProvider = stateManager.getGlobalSettingsKey("planModeApiProvider")
			if (!planProvider) {
				const planModelKey = getProviderModelIdKey(providerId as ApiProvider, "plan")
				if (planModelKey) config[planModelKey] = finalModelId
			}
		}

		// Add API key if provided (shared across roles, not role-specific)
	if (apiKey) {
		const keyField = ProviderToApiKeyMap[providerId as keyof typeof ProviderToApiKeyMap]
		if (keyField) {
			const fields = Array.isArray(keyField) ? keyField : [keyField]
			config[fields[0]] = apiKey
		}
	}

	// Add base URL if provided (shared across roles)
	if (baseUrl) {
		let normalizedBaseUrl = baseUrl.trim()
		if (normalizedBaseUrl) {
			normalizedBaseUrl = normalizedBaseUrl.replace(/\/chat\/completions\/?$/, "")
			normalizedBaseUrl = normalizedBaseUrl.replace(/\/+$/, "")
		}
		config.openAiBaseUrl = normalizedBaseUrl
	}

	if (azureApiVersion) {
		config.azureApiVersion = azureApiVersion
	}

	// Save via StateManager
	stateManager.setApiConfiguration(config)
	await stateManager.flushPendingState()

	// Rebuild API handler on active task if one exists
	if (controller?.task) {
		const currentMode = stateManager.getGlobalSettingsKey("mode")
		const apiConfig = stateManager.getApiConfiguration()
		controller.task.api = buildApiHandler({ ...apiConfig, ulid: controller.task.ulid }, currentMode)
	}
}

export interface ApplyBedrockConfigOptions {
	bedrockConfig: BedrockConfig
	role?: ModelRole
	modelId?: string
	customModelBaseId?: string
	controller?: Controller
}

/**
 * Apply Bedrock provider configuration to state.
 * When role is specified, writes config for that role only.
 */
export async function applyBedrockConfig(options: ApplyBedrockConfigOptions): Promise<void> {
	const { bedrockConfig, role = "act", modelId, customModelBaseId, controller } = options
	const stateManager = StateManager.get()

	const config: Record<string, unknown> = {
		[getRoleStateKey(role, "provider")]: "bedrock",
		awsAuthentication: bedrockConfig.awsAuthentication,
		awsRegion: bedrockConfig.awsRegion,
		awsUseCrossRegionInference: bedrockConfig.awsUseCrossRegionInference,
	}

	// For act role, also set plan if plan has no explicit provider
	if (role === "act") {
		const planProvider = stateManager.getGlobalSettingsKey("planModeApiProvider")
		if (!planProvider) {
			config["planModeApiProvider"] = "bedrock"
		}
	}

	// Add model ID
	const finalModelId = modelId || getDefaultModelId("bedrock")
	if (finalModelId) {
		const modelKey = getProviderModelIdKey("bedrock" as ApiProvider, role)
		if (modelKey) config[modelKey] = finalModelId

		if (role === "act") {
			const planProvider = stateManager.getGlobalSettingsKey("planModeApiProvider")
			if (!planProvider) {
				const planModelKey = getProviderModelIdKey("bedrock" as ApiProvider, "plan")
				if (planModelKey) config[planModelKey] = finalModelId
			}
		}
	}

	// Handle custom model (Application Inference Profile ARN)
	if (customModelBaseId) {
		config[getRoleStateKey(role, "awsBedrockCustomSelected")] = true
		config[getRoleStateKey(role, "awsBedrockCustomModelBaseId")] = customModelBaseId
	} else {
		config[getRoleStateKey(role, "awsBedrockCustomSelected")] = false
	}

	// Add optional AWS credentials
	if (bedrockConfig.awsProfile !== undefined) config.awsProfile = bedrockConfig.awsProfile
	if (bedrockConfig.awsAccessKey) config.awsAccessKey = bedrockConfig.awsAccessKey
	if (bedrockConfig.awsSecretKey) config.awsSecretKey = bedrockConfig.awsSecretKey
	if (bedrockConfig.awsSessionToken) config.awsSessionToken = bedrockConfig.awsSessionToken

	// Save via StateManager
	stateManager.setApiConfiguration(config as Record<string, string>)
	await stateManager.flushPendingState()

	// Rebuild API handler on active task if one exists
	if (controller?.task) {
		const currentMode = stateManager.getGlobalSettingsKey("mode")
		const apiConfig = stateManager.getApiConfiguration()
		controller.task.api = buildApiHandler({ ...apiConfig, ulid: controller.task.ulid }, currentMode)
	}
}
