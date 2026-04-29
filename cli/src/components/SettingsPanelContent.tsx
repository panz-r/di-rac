/**
 * Settings panel content for inline display in ChatView
 * Uses a tabbed interface: API, Auto Approve, Features, Other
 */

import type { AutoApprovalSettings } from "@shared/AutoApprovalSettings"
import { DEFAULT_AUTO_APPROVAL_SETTINGS } from "@shared/AutoApprovalSettings"
import type { ApiProvider, ModelInfo } from "@shared/api"
import { getProviderModelIdKey, isSettingsKey, ProviderToApiKeyMap } from "@shared/storage"
import { isOpenaiReasoningEffort, OPENAI_REASONING_EFFORT_OPTIONS, type OpenaiReasoningEffort } from "@shared/storage/types"
import type { TelemetrySetting } from "@shared/TelemetrySetting"
import { Box, Text, useInput } from "ink"
import Spinner from "ink-spinner"
import React, { useCallback, useEffect, useMemo, useState } from "react"
import { buildApiHandler } from "@/core/api"
import type { Controller } from "@/core/controller"
import { StateManager } from "@/core/storage/StateManager"
import { openAiCodexOAuthManager } from "@/integrations/openai-codex/oauth"
import { openExternal } from "@/utils/env"
import { supportsReasoningEffortForModel } from "@/utils/model-utils"
import { version as CLI_VERSION } from "../../package.json"
import { COLORS } from "../constants/colors"
import { useStdinContext } from "../context/StdinContext"
import { isMouseEscapeSequence } from "../utils/input"
import { applyBedrockConfig, applyProviderConfig } from "../utils/provider-config"
import { ApiKeyInput } from "./ApiKeyInput"
import { BedrockCustomModelFlow } from "./BedrockCustomModelFlow"
import { type BedrockConfig, BedrockSetup } from "./BedrockSetup"
import { Checkbox } from "./Checkbox"
import { LanguagePicker } from "./LanguagePicker"
import { CUSTOM_MODEL_ID, hasModelPicker, ModelPicker } from "./ModelPicker"
import { Panel, PanelTab } from "./Panel"
import { getProviderLabel, ProviderPicker } from "./ProviderPicker"

interface SettingsPanelContentProps {
	onClose: () => void
	controller?: Controller
	initialMode?: "model-picker" | "featured-models"
	initialModelKey?: "actModelId" | "planModelId"
}

type SettingsTab = "api" | "auto-approve" | "features" | "other"

interface ListItem {
	key: string
	label: string
	type: "checkbox" | "readonly" | "editable" | "separator" | "header" | "spacer" | "action" | "cycle"
	value: string | boolean
	description?: string
	isSubItem?: boolean
	parentKey?: string
}

function normalizeReasoningEffort(value: unknown): OpenaiReasoningEffort {
	if (isOpenaiReasoningEffort(value)) {
		return value
	}
	return "low"
}

function nextReasoningEffort(current: OpenaiReasoningEffort): OpenaiReasoningEffort {
	const idx = OPENAI_REASONING_EFFORT_OPTIONS.indexOf(current)
	return OPENAI_REASONING_EFFORT_OPTIONS[(idx + 1) % OPENAI_REASONING_EFFORT_OPTIONS.length]
}

const TABS: PanelTab[] = [
	{ key: "api", label: "API" },
	{ key: "auto-approve", label: "Auto-approve" },
	{ key: "features", label: "Features" },
	{ key: "other", label: "Other" },
]

// Settings configuration for simple boolean toggles
const FEATURE_SETTINGS = {
	subagents: {
		stateKey: "subagentsEnabled",
		default: false,
		label: "Subagents",
		description: "Let Dirac run focused subagents in parallel to explore the codebase for you",
	},
	autoCondense: {
		stateKey: "useAutoCondense",
		default: false,
		label: "Auto-condense",
		description: "Automatically summarize long conversations",
	},
	webTools: {
		stateKey: "diracWebToolsEnabled",
		default: true,
		label: "Web tools",
		description: "Enable web search and fetch tools",
	},
	strictPlanMode: {
		stateKey: "strictPlanModeEnabled",
		default: true,
		label: "Strict plan mode",
		description: "Require explicit mode switching",
	},
	parallelToolCalling: {
		stateKey: "enableParallelToolCalling",
		default: false,
		label: "Parallel tool calling",
		description: "Allow multiple tools in a single response",
	},
	doubleCheckCompletion: {
		stateKey: "doubleCheckCompletionEnabled",
		default: false,
		label: "Double-check completion",
		description: "Reject first completion attempt and require re-verification",
	},
} as const

type FeatureKey = keyof typeof FEATURE_SETTINGS

export const SettingsPanelContent: React.FC<SettingsPanelContentProps> = ({
	onClose,
	controller,
	initialMode,
	initialModelKey,
}) => {
	const { isRawModeSupported } = useStdinContext()
	const stateManager = StateManager.get()

	// UI state
	const [currentTab, setCurrentTab] = useState<SettingsTab>("api")
	const [selectedIndex, setSelectedIndex] = useState(0)
	const [isEditing, setIsEditing] = useState(false)
	const [isPickingModel, setIsPickingModel] = useState(initialMode === "model-picker")
	const [pickingModelKey, setPickingModelKey] = useState<"actModelId" | "planModelId" | null>(
		initialMode ? (initialModelKey ?? "actModelId") : null,
	)
	const [isPickingProvider, setIsPickingProvider] = useState(false)
	const [isPickingLanguage, setIsPickingLanguage] = useState(false)
	const [isEnteringApiKey, setIsEnteringApiKey] = useState(false)
	const [pendingProvider, setPendingProvider] = useState<string | null>(null)
	const [isConfiguringBedrock, setIsConfiguringBedrock] = useState(false)
	const [isWaitingForCodexAuth, setIsWaitingForCodexAuth] = useState(false)
	const [codexAuthUrl, setCodexAuthUrl] = useState<string | null>(null)
	const [codexAuthError, setCodexAuthError] = useState<string | null>(null)
	const [openAiCodexIsAuthenticated, setOpenAiCodexIsAuthenticated] = useState(false)
	const [openAiCodexEmail, setOpenAiCodexEmail] = useState<string | undefined>(undefined)
	const [apiKeyValue, setApiKeyValue] = useState("")
	const [editValue, setEditValue] = useState("")

	// Bedrock custom ARN flow state
	const [isBedrockCustomFlow, setIsBedrockCustomFlow] = useState(false)

	// Settings state - single object for feature toggles
	const [features, setFeatures] = useState<Record<FeatureKey, boolean>>(() => {
		const initial: Record<string, boolean> = {}
		for (const [key, config] of Object.entries(FEATURE_SETTINGS)) {
			if (isSettingsKey(config.stateKey)) {
				initial[key] = stateManager.getGlobalSettingsKey(config.stateKey)
			} else {
				initial[key] = stateManager.getGlobalStateKey(config.stateKey)
			}
		}
		return initial as Record<FeatureKey, boolean>
	})

	// API tab state
	const [separateModels, setSeparateModels] = useState<boolean>(
		() => stateManager.getGlobalSettingsKey("planActSeparateModelsSetting") ?? false,
	)
	// Thinking is enabled if budget > 0
	const [actThinkingEnabled, setActThinkingEnabled] = useState<boolean>(
		() => (stateManager.getGlobalSettingsKey("actModeThinkingBudgetTokens") ?? 0) > 0,
	)
	const [planThinkingEnabled, setPlanThinkingEnabled] = useState<boolean>(
		() => (stateManager.getGlobalSettingsKey("planModeThinkingBudgetTokens") ?? 0) > 0,
	)
	const [actReasoningEffort, setActReasoningEffort] = useState<OpenaiReasoningEffort>(() =>
		normalizeReasoningEffort(stateManager.getGlobalSettingsKey("actModeReasoningEffort")),
	)
	const [planReasoningEffort, setPlanReasoningEffort] = useState<OpenaiReasoningEffort>(() =>
		normalizeReasoningEffort(stateManager.getGlobalSettingsKey("planModeReasoningEffort")),
	)

	// Auto-approve settings (complex nested object)
	const [autoApproveSettings, setAutoApproveSettings] = useState<AutoApprovalSettings>(() => {
		return stateManager.getGlobalSettingsKey("autoApprovalSettings") ?? DEFAULT_AUTO_APPROVAL_SETTINGS
	})

	// Other tab state
	const [preferredLanguage, setPreferredLanguage] = useState<string>(
		() => stateManager.getGlobalSettingsKey("preferredLanguage") || "English",
	)
	const [telemetry, setTelemetry] = useState<TelemetrySetting>(
		() => stateManager.getGlobalSettingsKey("telemetrySetting") || "unset",
	)

	// Get current provider and model info
	const [provider, setProvider] = useState<string>(
		() =>
			stateManager.getApiConfiguration().actModeApiProvider ||
			stateManager.getApiConfiguration().planModeApiProvider ||
			"not configured",
	)
	// Refresh trigger to force re-reading model IDs from state
	const [modelRefreshKey, setModelRefreshKey] = useState(0)
	const refreshModelIds = useCallback(() => setModelRefreshKey((k) => k + 1), [])
	// Read model IDs from state (re-reads when refreshKey changes)
	// Update OpenAI Codex auth status
	useEffect(() => {
		const updateAuthStatus = async () => {
			const isAuthenticated = await openAiCodexOAuthManager.isAuthenticated()
			setOpenAiCodexIsAuthenticated(isAuthenticated)
			if (isAuthenticated) {
				const email = await openAiCodexOAuthManager.getEmail()
				setOpenAiCodexEmail(email ?? undefined)
			} else {
				setOpenAiCodexEmail(undefined)
			}
		}
		updateAuthStatus()
	}, [provider, isWaitingForCodexAuth])


	const { actModelId, planModelId } = useMemo(() => {
		const apiConfig = stateManager.getApiConfiguration()
		const actProvider = apiConfig.actModeApiProvider
		const planProvider = apiConfig.planModeApiProvider || actProvider
		if (!actProvider && !planProvider) {
			return { actModelId: "", planModelId: "" }
		}
		const actKey = actProvider ? getProviderModelIdKey(actProvider, "act") : null
		const planKey = planProvider ? getProviderModelIdKey(planProvider, "plan") : null
		return {
			actModelId: actKey ? (stateManager.getGlobalSettingsKey(actKey) as string) || "" : "",
			planModelId: planKey ? (stateManager.getGlobalSettingsKey(planKey) as string) || "" : "",
		}
	}, [modelRefreshKey, stateManager])

	// Toggle a feature setting
	const toggleFeature = useCallback(
		(key: FeatureKey) => {
			const config = FEATURE_SETTINGS[key]
			const newValue = !features[key]
			setFeatures((prev) => ({ ...prev, [key]: newValue }))
			stateManager.setGlobalState(config.stateKey, newValue)
		},
		[features, stateManager],
	)

	// Build items list based on current tab
	const items: ListItem[] = useMemo(() => {
		// Some providers/models expose reasoning effort instead of thinking budget controls.
		const providerUsesReasoningEffort = provider === "openai-native" || provider === "openai-codex"
		const showActReasoningEffort = supportsReasoningEffortForModel(actModelId || "")
		const showPlanReasoningEffort = supportsReasoningEffortForModel(planModelId || "")
		const showActThinkingOption = !providerUsesReasoningEffort && !showActReasoningEffort
		const showPlanThinkingOption = !providerUsesReasoningEffort && !showPlanReasoningEffort

		switch (currentTab) {
			case "api":
				return [
					{
						key: "provider",
						label: "Provider",
						type: "editable",
						value: provider ? getProviderLabel(provider) : "not configured",
					},
					...(provider === "zai"
						? [
								{
									key: "zaiApiLine",
									label: "Entrypoint",
									type: "cycle" as const,
									value: {
										international: "api.z.ai",
										"coding-plan": "Coding Plan",
										china: "open.bigmodel.cn",
									}[(stateManager.getGlobalSettingsKey("zaiApiLine") as string) || "international"] || "api.z.ai",
								},
							]
						: []),
					...(provider === "openai-codex" && openAiCodexIsAuthenticated
						? [
								{
									key: "codexEmail",
									label: "Authenticated as",
									type: "readonly" as const,
									value: openAiCodexEmail || "ChatGPT User",
								},
								{
									key: "codexSignOut",
									label: "Sign Out",
									type: "action" as const,
									value: "",
								},
							]
						: []),
					...(separateModels
						? [
								{ key: "spacer0", label: "", type: "spacer" as const, value: "" },
								{ key: "actHeader", label: "Act Mode", type: "header" as const, value: "" },
								{
									key: "actModelId",
									label: "Model ID",
									type: "editable" as const,
									value: actModelId || "not set",
								},
								...(showActThinkingOption
									? [
											{
												key: "actThinkingEnabled",
												label: "Enable thinking",
												type: "checkbox" as const,
												value: actThinkingEnabled,
											},
										]
									: []),
								...(showActReasoningEffort
									? [
											{
												key: "actReasoningEffort",
												label: "Reasoning effort",
												type: "cycle" as const,
												value: actReasoningEffort,
											},
										]
									: []),
								{ key: "planHeader", label: "Plan Mode", type: "header" as const, value: "" },
								{
									key: "planModelId",
									label: "Model ID",
									type: "editable" as const,
									value: planModelId || "not set",
								},
								...(showPlanThinkingOption
									? [
											{
												key: "planThinkingEnabled",
												label: "Enable thinking",
												type: "checkbox" as const,
												value: planThinkingEnabled,
											},
										]
									: []),
								...(showPlanReasoningEffort
									? [
											{
												key: "planReasoningEffort",
												label: "Reasoning effort",
												type: "cycle" as const,
												value: planReasoningEffort,
											},
										]
									: []),
								{ key: "spacer1", label: "", type: "spacer" as const, value: "" },
							]
						: [
								{
									key: "actModelId",
									label: "Model ID",
									type: "editable" as const,
									value: actModelId || "not set",
								},
								...(showActThinkingOption
									? [
											{
												key: "actThinkingEnabled",
												label: "Enable thinking",
												type: "checkbox" as const,
												value: actThinkingEnabled,
											},
										]
									: []),
								...(showActReasoningEffort
									? [
											{
												key: "actReasoningEffort",
												label: "Reasoning effort",
												type: "cycle" as const,
												value: actReasoningEffort,
											},
										]
									: []),
							]),
					{
						key: "separateModels",
						label: "Use separate models for Plan and Act",
						type: "checkbox",
						value: separateModels,
					},
				]

			case "auto-approve": {
				const result: ListItem[] = []
				const actions = autoApproveSettings.actions

				// Helper to add parent/child checkbox pairs
				const addActionPair = (
					parentKey: string,
					parentLabel: string,
					parentDesc: string,
					childKey: string,
					childLabel: string,
					childDesc: string,
				) => {
					result.push({
						key: parentKey,
						label: parentLabel,
						type: "checkbox",
						value: actions[parentKey as keyof typeof actions] ?? false,
						description: parentDesc,
					})
					if (actions[parentKey as keyof typeof actions]) {
						result.push({
							key: childKey,
							label: childLabel,
							type: "checkbox",
							value: actions[childKey as keyof typeof actions] ?? false,
							description: childDesc,
							isSubItem: true,
							parentKey,
						})
					}
				}

				addActionPair(
					"readFiles",
					"Read and analyze files",
					"Read and analyze files in the working directory",
					"readFilesExternally",
					"Read all files",
					"Read files outside working directory",
				)
				addActionPair(
					"editFiles",
					"Edit and create files",
					"Edit and create files in the working directory",
					"editFilesExternally",
					"Edit all files",
					"Edit files outside working directory",
				)
				result.push({
					key: "executeCommands",
					label: "Auto-approve safe commands",
					type: "checkbox",
					value: actions.executeCommands ?? false,
					description: "Run harmless terminal commands automatically",
				})

				result.push(
					{
						key: "useBrowser",
						label: "Use the browser",
						type: "checkbox",
						value: actions.useBrowser,
						description: "Browse and interact with web pages",
					},
					{ key: "separator", label: "", type: "separator", value: false },
					{
						key: "enableNotifications",
						label: "Enable notifications",
						type: "checkbox",
						value: autoApproveSettings.enableNotifications,
						description: "System alerts when Dirac needs your attention",
					},
				)
				return result
			}

			case "features":
				return Object.entries(FEATURE_SETTINGS).map(([key, config]) => ({
					key,
					label: config.label,
					type: "checkbox" as const,
					value: features[key as FeatureKey],
					description: config.description,
				}))

			case "other":
				return [
					{ key: "language", label: "Preferred language", type: "editable", value: preferredLanguage },
					{
						key: "telemetry",
						label: "Error/usage reporting",
						type: "checkbox",
						value: telemetry !== "disabled",
						description: "Help improve Dirac by sending anonymous usage data",
					},
					{ key: "separator", label: "", type: "separator", value: "" },
					{ key: "version", label: "", type: "readonly", value: `Dirac v${CLI_VERSION}` },
				]

			default:
				return []
		}
	}, [
		currentTab,
		modelRefreshKey,
		provider,
		actModelId,
		planModelId,
		separateModels,
		actThinkingEnabled,
		planThinkingEnabled,
		actReasoningEffort,
		planReasoningEffort,
		autoApproveSettings,
		features,
		preferredLanguage,
		telemetry,
	])

	// Reset selection when changing tabs
	const handleTabChange = useCallback((tabKey: string) => {
		setCurrentTab(tabKey as SettingsTab)
		setSelectedIndex(0)
		setIsEditing(false)
		setIsPickingModel(false)
		setPickingModelKey(null)
		setIsPickingProvider(false)
		setIsPickingLanguage(false)
		setIsEnteringApiKey(false)
		setPendingProvider(null)
		setApiKeyValue("")
	}, [])

	// Ensure selected index is valid when items change
	useEffect(() => {
		if (selectedIndex >= items.length) {
			setSelectedIndex(Math.max(0, items.length - 1))
		}
	}, [items.length, selectedIndex])

	const rebuildTaskApi = useCallback(() => {
		if (!controller?.task) {
			return
		}
		const currentMode = stateManager.getGlobalSettingsKey("mode")
		const apiConfig = stateManager.getApiConfiguration()
		controller.task.api = buildApiHandler({ ...apiConfig, ulid: controller.task.ulid }, currentMode)
	}, [controller, stateManager])

	const setReasoningEffortForMode = useCallback(
		(mode: "act" | "plan", effort: OpenaiReasoningEffort) => {
			if (mode === "act") {
				setActReasoningEffort(effort)
				stateManager.setGlobalState("actModeReasoningEffort", effort)
				if (!separateModels) {
					setPlanReasoningEffort(effort)
					stateManager.setGlobalState("planModeReasoningEffort", effort)
				}
			} else {
				setPlanReasoningEffort(effort)
				stateManager.setGlobalState("planModeReasoningEffort", effort)
			}
			rebuildTaskApi()
		},
		[separateModels, rebuildTaskApi, stateManager],
	)

	// Handle toggle/edit for selected item
	const handleAction = useCallback(() => {
		const item = items[selectedIndex]
		if (!item || item.type === "readonly" || item.type === "separator" || item.type === "header" || item.type === "spacer")
			return

		if (item.type === "action") {
			return
		}

		if (item.type === "cycle") {
			if (item.key === "zaiApiLine") {
				const current = (stateManager.getGlobalSettingsKey("zaiApiLine") as string) || "international"
				const lines = ["international", "coding-plan", "china"]
				const next = lines[(lines.indexOf(current) + 1) % lines.length]
				stateManager.setGlobalState("zaiApiLine", next)
				stateManager.flushPendingState()
				setModelRefreshKey((k) => k + 1)
				if (controller?.task) {
					const mode = stateManager.getGlobalSettingsKey("mode")
					const apiConfig = stateManager.getApiConfiguration()
					controller.task.api = buildApiHandler({ ...apiConfig, ulid: controller.task.ulid }, mode)
				}
				return
			}
			const targetMode = item.key === "actReasoningEffort" ? "act" : item.key === "planReasoningEffort" ? "plan" : undefined
			if (targetMode) {
				const currentEffort = targetMode === "act" ? actReasoningEffort : planReasoningEffort
				setReasoningEffortForMode(targetMode, nextReasoningEffort(currentEffort))
			}
			return
		}

		if (item.type === "editable") {
			// For provider field, use the provider picker
			if (item.key === "provider") {
				setIsPickingProvider(true)
				return
			}
			// For model ID fields, check if we should use the model picker
			if ((item.key === "actModelId" || item.key === "planModelId") && hasModelPicker(provider)) {
				setPickingModelKey(item.key as "actModelId" | "planModelId")
				setIsPickingModel(true)
				return
			}
			// For language field, use the language picker
			if (item.key === "language") {
				setIsPickingLanguage(true)
				return
			}
			setEditValue(typeof item.value === "string" ? item.value : "")
			setIsEditing(true)
			return
		}

		// Checkbox handling
		const newValue = !item.value

		// Feature settings (simple toggles)
		if (item.key in FEATURE_SETTINGS) {
			toggleFeature(item.key as FeatureKey)
			return
		}

		// API tab
		if (item.key === "separateModels") {
			setSeparateModels(newValue)
			stateManager.setGlobalState("planActSeparateModelsSetting", newValue)
			// When disabling separate models, sync plan model to act model
			if (!newValue) {
				const apiConfig = stateManager.getApiConfiguration()
				const actProvider = apiConfig.actModeApiProvider
				const planProvider = apiConfig.planModeApiProvider || actProvider
				if (actProvider) {
					const actKey = getProviderModelIdKey(actProvider, "act")
					const planKey = planProvider ? getProviderModelIdKey(planProvider, "plan") : null
					const actModel = stateManager.getGlobalSettingsKey(actKey)
					if (planKey) stateManager.setGlobalState(planKey, actModel)
				}
				const actThinkingBudget = stateManager.getGlobalSettingsKey("actModeThinkingBudgetTokens") ?? 0
				stateManager.setGlobalState("planModeThinkingBudgetTokens", actThinkingBudget)
				setPlanThinkingEnabled(actThinkingBudget > 0)

				const actEffort = normalizeReasoningEffort(stateManager.getGlobalSettingsKey("actModeReasoningEffort"))
				stateManager.setGlobalState("planModeReasoningEffort", actEffort)
				setPlanReasoningEffort(actEffort)
			}

			rebuildTaskApi()
			return
		}

		// Thinking toggles - set budget to 1024 when enabled, 0 when disabled
		if (item.key === "codexSignOut") {
			openAiCodexOAuthManager.clearCredentials().then(() => {
				setOpenAiCodexIsAuthenticated(false)
				setOpenAiCodexEmail(undefined)
				rebuildTaskApi()
			})
			return
		}


		if (item.key === "actThinkingEnabled") {
			setActThinkingEnabled(newValue)
			stateManager.setGlobalState("actModeThinkingBudgetTokens", newValue ? 1024 : 0)
			if (!separateModels) {
				setPlanThinkingEnabled(newValue)
				stateManager.setGlobalState("planModeThinkingBudgetTokens", newValue ? 1024 : 0)
			}
			// Rebuild API handler to apply thinking budget change
			rebuildTaskApi()
			return
		}
		if (item.key === "planThinkingEnabled") {
			setPlanThinkingEnabled(newValue)
			stateManager.setGlobalState("planModeThinkingBudgetTokens", newValue ? 1024 : 0)
			// Rebuild API handler to apply thinking budget change
			rebuildTaskApi()
			return
		}

		// Other tab
		if (item.key === "telemetry") {
			const newTelemetry: TelemetrySetting = newValue ? "enabled" : "disabled"
			setTelemetry(newTelemetry)
			stateManager.setGlobalState("telemetrySetting", newTelemetry)
			// Flush synchronously before continuing - must complete before app can exit
			void stateManager.flushPendingState().then(() => {
				// Update telemetry providers to respect the new setting
				controller?.updateTelemetrySetting(newTelemetry)
			})
			return
		}

		// Auto-approve actions
		if (item.key === "enableNotifications") {
			const newSettings = {
				...autoApproveSettings,
				version: (autoApproveSettings.version ?? 1) + 1,
				enableNotifications: newValue,
			}
			setAutoApproveSettings(newSettings)
			stateManager.setGlobalState("autoApprovalSettings", newSettings)
			return
		}

		// Auto-approve action toggles
		const actionKey = item.key as keyof AutoApprovalSettings["actions"]
		const newActions = { ...autoApproveSettings.actions, [actionKey]: newValue }

		// If disabling a parent, also disable its children
		if (!newValue) {
			if (actionKey === "readFiles") newActions.readFilesExternally = false
			if (actionKey === "editFiles") newActions.editFilesExternally = false
		}

		// If enabling a child, also enable its parent
		if (newValue && item.parentKey) {
			newActions[item.parentKey as keyof typeof newActions] = true
		}

		const newSettings = { ...autoApproveSettings, version: (autoApproveSettings.version ?? 1) + 1, actions: newActions }
		setAutoApproveSettings(newSettings)
		stateManager.setGlobalState("autoApprovalSettings", newSettings)
	}, [
		items,
		selectedIndex,
		stateManager,
		autoApproveSettings,
		toggleFeature,
		separateModels,
		actReasoningEffort,
		planReasoningEffort,
		rebuildTaskApi,
		setReasoningEffortForMode,
	])

	// Handle completion of the Bedrock custom ARN flow (ARN + base model selected)
	const handleBedrockCustomFlowComplete = useCallback(
		async (arn: string, baseModelId: string) => {
			if (!pickingModelKey) return
			const apiConfig = stateManager.getApiConfiguration()

			// Build a minimal BedrockConfig from current state for applyBedrockConfig
			const bedrockConfig: BedrockConfig = {
				awsRegion: apiConfig.awsRegion ?? "us-east-1",
				awsAuthentication: apiConfig.awsUseProfile ? "profile" : "credentials",
				awsUseCrossRegionInference: Boolean(apiConfig.awsUseCrossRegionInference),
			}

			await applyBedrockConfig({
				bedrockConfig,
				modelId: arn,
				customModelBaseId: baseModelId,
				controller,
			})

			// Flush pending state to ensure everything is persisted
			await stateManager.flushPendingState()

			// Rebuild API handler if there's an active task
			rebuildTaskApi()

			refreshModelIds()
			setIsBedrockCustomFlow(false)
			setPickingModelKey(null)

			// If opened from /models command, close the entire settings panel
			if (initialMode) {
				onClose()
			}
		},
		[pickingModelKey, stateManager, controller, rebuildTaskApi, refreshModelIds, initialMode, onClose],
	)

	// Handle model selection from picker
	const handleModelSelect = useCallback(
		async (modelId: string) => {
			if (!pickingModelKey) return

			// Intercept "Custom" selection for Bedrock — redirect to custom ARN input flow
			if (modelId === CUSTOM_MODEL_ID && provider === "bedrock") {
				setIsPickingModel(false)
				setIsBedrockCustomFlow(true)
				return
			}

			const apiConfig = stateManager.getApiConfiguration()
			const actProvider = apiConfig.actModeApiProvider
			const planProvider = apiConfig.planModeApiProvider || actProvider
			const providerForSelection = separateModels
				? pickingModelKey === "actModelId"
					? actProvider
					: planProvider
				: actProvider || planProvider
			if (!providerForSelection) return
			// Use provider-specific model ID keys (e.g., dirac uses actModeOpenRouterModelId)
			const actKey = actProvider ? getProviderModelIdKey(actProvider, "act") : null
			const planKey = planProvider ? getProviderModelIdKey(planProvider, "plan") : null

			// For dirac/openrouter providers, also set model info (like webview does)
			let modelInfo: ModelInfo | undefined
			if (providerForSelection === "dirac" || providerForSelection === "openrouter") {
				const openRouterModels = await controller?.readOpenRouterModels()
				modelInfo = openRouterModels?.[modelId]
			}

			if (separateModels) {
				// Only update the selected mode's model
				const stateKey = pickingModelKey === "actModelId" ? actKey : planKey
				if (stateKey) stateManager.setGlobalState(stateKey, modelId)
				// Set model info for the selected mode
				if (modelInfo) {
					const infoKey =
						pickingModelKey === "actModelId" ? "actModeOpenRouterModelInfo" : "planModeOpenRouterModelInfo"
					stateManager.setGlobalState(infoKey, modelInfo)
				}
			} else {
				// Update both modes to keep them in sync
				if (actKey) stateManager.setGlobalState(actKey, modelId)
				if (planKey) stateManager.setGlobalState(planKey, modelId)
				// Set model info for both modes
				if (modelInfo) {
					stateManager.setGlobalState("actModeOpenRouterModelInfo", modelInfo)
					stateManager.setGlobalState("planModeOpenRouterModelInfo", modelInfo)
				}
			}

			// Flush pending state to ensure model ID is persisted
			await stateManager.flushPendingState()

			// Rebuild API handler if there's an active task
			if (controller?.task) {
				const currentMode = stateManager.getGlobalSettingsKey("mode")
				const freshApiConfig = stateManager.getApiConfiguration()
				controller.task.api = buildApiHandler({ ...freshApiConfig, ulid: controller.task.ulid }, currentMode)
			}

			refreshModelIds()
			setIsPickingModel(false)
			setPickingModelKey(null)

			// If opened from /models command, close the entire settings panel
			if (initialMode) {
				onClose()
			}
		},
		[pickingModelKey, separateModels, stateManager, controller, provider, refreshModelIds, initialMode, onClose],
	)

	// Handle language selection from picker
	const handleLanguageSelect = useCallback(
		(language: string) => {
			setPreferredLanguage(language)
			stateManager.setGlobalState("preferredLanguage", language)
			setIsPickingLanguage(false)
		},
		[stateManager],
	)

	// Handle OpenAI Codex OAuth flow
	const startCodexAuth = useCallback(async () => {
		try {
			setIsWaitingForCodexAuth(true)
			setCodexAuthError(null)

			// Get the authorization URL and start the callback server
			const authUrl = openAiCodexOAuthManager.startAuthorizationFlow()
			setCodexAuthUrl(authUrl)

			// Open browser to authorization URL
			await openExternal(authUrl)

			// Wait for the callback
			await openAiCodexOAuthManager.waitForCallback()

			// Success - apply provider config
			await applyProviderConfig({ providerId: "openai-codex", controller })
			setProvider("openai-codex")
			refreshModelIds()
			setIsWaitingForCodexAuth(false)
			setCodexAuthUrl(null)
		} catch (error) {
			openAiCodexOAuthManager.cancelAuthorizationFlow()
			setCodexAuthError(error instanceof Error ? error.message : String(error))
			setIsWaitingForCodexAuth(false)
			setCodexAuthUrl(null)
		}
	}, [controller])

	const handleProviderSelect = useCallback(
		async (providerId: string) => {
			// Special handling for Dirac - uses OAuth (but skip if already logged in)
			if (providerId === "dirac") {
				setIsPickingProvider(false)
				await applyProviderConfig({ providerId: "dirac", controller })
				setProvider("dirac")
				refreshModelIds()
				return
			}

			// Special handling for OpenAI Codex - uses OAuth instead of API key
			if (providerId === "openai-codex") {
				setIsPickingProvider(false)
				startCodexAuth()
				return
			}

			// Special handling for Bedrock - needs multi-field configuration
			if (providerId === "bedrock") {
				setPendingProvider(providerId)
				setIsPickingProvider(false)
				setIsConfiguringBedrock(true)
				return
			}

			// Check if this provider needs an API key
			const keyField = ProviderToApiKeyMap[providerId as ApiProvider]
			if (keyField) {
				// Provider needs an API key - go to API key entry mode
				// Pre-fill with existing key if configured
				const apiConfig = stateManager.getApiConfiguration()
				const fieldName = Array.isArray(keyField) ? keyField[0] : keyField
				const existingKey = (apiConfig as Record<string, string>)[fieldName] || ""
				setPendingProvider(providerId)
				setApiKeyValue(existingKey)
				setIsPickingProvider(false)
				setIsEnteringApiKey(true)
			} else {
				// Provider doesn't need an API key (rare) - just set it
				await applyProviderConfig({ providerId, controller })
				setProvider(providerId)
				refreshModelIds()
				setIsPickingProvider(false)
			}
		},
		[stateManager, startCodexAuth, controller, refreshModelIds],
	)

	// Handle API key submission after provider selection
	const handleApiKeySubmit = useCallback(
		async (submittedValue: string) => {
			if (!pendingProvider || !submittedValue.trim()) {
				return
			}

			await applyProviderConfig({ providerId: pendingProvider, apiKey: submittedValue.trim(), controller })
			setProvider(pendingProvider)
			refreshModelIds()
			setIsEnteringApiKey(false)
			setPendingProvider(null)
			setApiKeyValue("")
		},
		[pendingProvider, controller, refreshModelIds],
	)

	// Handle Bedrock configuration complete
	const handleBedrockComplete = useCallback(
		(bedrockConfig: BedrockConfig) => {
			// Update UI state first for responsiveness
			setProvider("bedrock")
			refreshModelIds()
			setIsConfiguringBedrock(false)
			setPendingProvider(null)

			// Apply config and rebuild API handler in background
			applyBedrockConfig({ bedrockConfig, controller })
		},
		[controller, refreshModelIds],
	)

	// Handle saving edited value
	const handleSave = useCallback(() => {
		const item = items[selectedIndex]
		if (!item) return

		switch (item.key) {
			case "actModelId":
			case "planModelId": {
				// Use provider-specific model ID keys (e.g., dirac uses actModeOpenRouterModelId)
				const apiConfig = stateManager.getApiConfiguration()
				const actProvider = apiConfig.actModeApiProvider
				const planProvider = apiConfig.planModeApiProvider || actProvider
				if (!actProvider && !planProvider) break
				const actKey = actProvider ? getProviderModelIdKey(actProvider, "act") : null
				const planKey = planProvider ? getProviderModelIdKey(planProvider, "plan") : null

				if (separateModels) {
					// Only update the selected mode's model
					const stateKey = item.key === "actModelId" ? actKey : planKey
					if (stateKey) stateManager.setGlobalState(stateKey, editValue || undefined)
				} else {
					// Update both modes to keep them in sync
					if (actKey) stateManager.setGlobalState(actKey, editValue || undefined)
					if (planKey) stateManager.setGlobalState(planKey, editValue || undefined)
				}

				stateManager.flushPendingState()
				refreshModelIds()
				rebuildTaskApi()
				break
			}
			case "language":
				setPreferredLanguage(editValue)
				stateManager.setGlobalState("preferredLanguage", editValue)
				break
		}
		setIsEditing(false)
	}, [items, selectedIndex, editValue, separateModels, stateManager, refreshModelIds, rebuildTaskApi])

	// Navigate to next/prev item, skipping non-interactive items
	const navigateItems = useCallback(
		(direction: "up" | "down") => {
			setSelectedIndex((i) => {
				let next = direction === "up" ? (i > 0 ? i - 1 : items.length - 1) : i < items.length - 1 ? i + 1 : 0

				// Skip separators, headers, and spacers
				const skipTypes = ["separator", "header", "spacer"]
				while (skipTypes.includes(items[next]?.type) && next !== i) {
					next = direction === "up" ? (next > 0 ? next - 1 : items.length - 1) : next < items.length - 1 ? next + 1 : 0
				}
				return next
			})
		},
		[items],
	)

	// Navigate tabs
	const navigateTabs = useCallback(
		(direction: "left" | "right") => {
			const tabKeys = TABS.map((t) => t.key)
			const currentIdx = tabKeys.indexOf(currentTab)
			const newIdx =
				direction === "left"
					? currentIdx > 0
						? currentIdx - 1
						: tabKeys.length - 1
					: currentIdx < tabKeys.length - 1
						? currentIdx + 1
						: 0
			handleTabChange(tabKeys[newIdx])
		},
		[currentTab, handleTabChange],
	)

	// Handle keyboard input
	// Disable when in modes where child components handle input
	useInput(
		(input, key) => {
			// Filter out mouse escape sequences
			if (isMouseEscapeSequence(input)) {
				return
			}

			// Provider picker mode - escape to close, input is handled by ProviderPicker
			if (isPickingProvider) {
				if (key.escape) {
					setIsPickingProvider(false)
				}
				return
			}


			// Model picker mode - escape to close, input is handled by ModelPicker
			if (isPickingModel) {
				if (key.escape) {
					setIsPickingModel(false)
					setPickingModelKey(null)
					// If opened from /models command, close the entire settings panel
					if (initialMode) {
						onClose()
					}
				}
				return
			}

			// Language picker mode - escape to close, input is handled by LanguagePicker
			if (isPickingLanguage) {
				if (key.escape) {
					setIsPickingLanguage(false)
				}
				return
			}

			// Codex OAuth waiting mode - escape to cancel
			if (isWaitingForCodexAuth) {
				if (key.escape) {
					openAiCodexOAuthManager.cancelAuthorizationFlow()
					setIsWaitingForCodexAuth(false)
				}
				return
			}

			// Codex OAuth error mode - any key to dismiss
			if (codexAuthError) {
				setCodexAuthError(null)
				return
			}

			// Organization picker mode - escape to close, input is handled by OrganizationPicker

			// Bedrock custom flow - input handled by BedrockCustomModelFlow component
			if (isBedrockCustomFlow) {
				return
			}

			if (isEditing) {
				if (key.escape) {
					setIsEditing(false)
					return
				}
				if (key.return) {
					handleSave()
					return
				}
				if (key.backspace || key.delete) {
					setEditValue((prev) => prev.slice(0, -1))
					return
				}
				if (input && !key.ctrl && !key.meta) {
					setEditValue((prev) => prev + input)
				}
				return
			}

			if (key.escape) {
				onClose()
				return
			}
			if (key.leftArrow) {
				navigateTabs("left")
				return
			}
			if (key.rightArrow) {
				navigateTabs("right")
				return
			}
			if (key.upArrow) {
				navigateItems("up")
				return
			}
			if (key.downArrow) {
				navigateItems("down")
				return
			}
			if (key.tab || key.return) {
				handleAction()
				return
			}
		},
		{ isActive: isRawModeSupported && !isEnteringApiKey && !isConfiguringBedrock },
	)

	// Render content
	const renderContent = () => {
		if (isPickingProvider) {
			return (
				<Box flexDirection="column">
					<Text bold color={COLORS.primaryBlue}>
						Select Provider
					</Text>
					<Box marginTop={1}>
						<ProviderPicker isActive={isPickingProvider} onSelect={handleProviderSelect} />
					</Box>
					<Box marginTop={1}>
						<Text color="gray">Type to search, arrows to navigate, Enter to select, Esc to cancel</Text>
					</Box>
				</Box>
			)
		}

		if (isEnteringApiKey && pendingProvider) {
			return (
				<ApiKeyInput
					isActive={isEnteringApiKey}
					onCancel={() => {
						setIsEnteringApiKey(false)
						setPendingProvider(null)
						setApiKeyValue("")
					}}
					onChange={setApiKeyValue}
					onSubmit={handleApiKeySubmit}
					providerName={getProviderLabel(pendingProvider)}
					value={apiKeyValue}
				/>
			)
		}

		if (isConfiguringBedrock) {
			return (
				<BedrockSetup
					isActive={isConfiguringBedrock}
					onCancel={() => {
						setIsConfiguringBedrock(false)
						setPendingProvider(null)
					}}
					onComplete={handleBedrockComplete}
				/>
			)
		}
		if (isWaitingForCodexAuth) {
			return (
				<Box flexDirection="column">
					<Box>
						<Text color={COLORS.primaryBlue}>
							<Spinner type="dots" />
						</Text>
						<Text color="white"> Waiting for ChatGPT sign-in...</Text>
					</Box>
					<Box marginTop={1}>
						<Text color="gray">Sign in with your ChatGPT account in the browser.</Text>
					</Box>
					{codexAuthUrl && (
						<Box flexDirection="column" marginTop={1}>
							<Text color="gray">If the browser didn't open, use this URL:</Text>
							<Text color="cyan" wrap="wrap">
								{codexAuthUrl}
							</Text>
							<Box marginTop={1}>
								<Text color="yellow">
									Note: If you are on a remote machine, you may need to set up SSH port forwarding:
								</Text>
							</Box>
							<Text color="gray">ssh -L 1455:localhost:1455 your-remote-host</Text>
						</Box>
					)}
					<Box marginTop={1}>
						<Text color="gray">Requires ChatGPT Plus, Pro, or Team subscription.</Text>
					</Box>
					<Box marginTop={1}>
						<Text color="gray">Esc to cancel</Text>
					</Box>
				</Box>
			)
		}

		if (codexAuthError) {
			return (
				<Box flexDirection="column">
					<Text bold color="red">
						ChatGPT sign-in failed
					</Text>
					<Box marginTop={1}>
						<Text color="yellow">{codexAuthError}</Text>
					</Box>
					<Box marginTop={1}>
						<Text color="gray">Press any key to continue</Text>
					</Box>
				</Box>
			)
		}


		if (isPickingModel && pickingModelKey) {
			const label = pickingModelKey === "actModelId" ? "Model ID (Act)" : "Model ID (Plan)"
			return (
				<Box flexDirection="column">
					<Text bold color={COLORS.primaryBlue}>
						Select: {label}
					</Text>
					<Box marginTop={1}>
						<ModelPicker
							controller={controller}
							isActive={isPickingModel}
							onChange={() => {}}
							onSubmit={handleModelSelect}
							provider={provider}
						/>
					</Box>
					<Box marginTop={1}>
						<Text color="gray">Type to search, arrows to navigate, Enter to select, Esc to cancel</Text>
					</Box>
				</Box>
			)
		}

		if (isPickingLanguage) {
			return (
				<Box flexDirection="column">
					<Text bold color={COLORS.primaryBlue}>
						Select Language
					</Text>
					<Box marginTop={1}>
						<LanguagePicker isActive={isPickingLanguage} onSelect={handleLanguageSelect} />
					</Box>
					<Box marginTop={1}>
						<Text color="gray">Type to search, arrows to navigate, Enter to select, Esc to cancel</Text>
					</Box>
				</Box>
			)
		}

		// Bedrock custom model flow (ARN input + base model selection)
		if (isBedrockCustomFlow) {
			return (
				<BedrockCustomModelFlow
					isActive={isBedrockCustomFlow}
					onCancel={() => {
						setIsBedrockCustomFlow(false)
						setIsPickingModel(true)
					}}
					onComplete={handleBedrockCustomFlowComplete}
				/>
			)
		}

		if (isEditing) {
			const item = items[selectedIndex]
			return (
				<Box flexDirection="column">
					<Text bold color={COLORS.primaryBlue}>
						Edit: {item?.label}
					</Text>
					<Box marginTop={1}>
						<Text color="white">{editValue}</Text>
						<Text color="gray">|</Text>
					</Box>
					<Text color="gray">Enter to save, Esc to cancel</Text>
				</Box>
			)
		}

		return (
			<Box flexDirection="column">
				{items.map((item, idx) => {
					const isSelected = idx === selectedIndex

					if (item.type === "header") {
						return (
							<Box key={item.key} marginTop={idx > 0 ? 0 : 0}>
								<Text bold color="white">
									{item.label}
								</Text>
							</Box>
						)
					}

					if (item.type === "spacer") {
						return <Box key={item.key} marginTop={1} />
					}

					if (item.type === "separator") {
						return (
							<Box
								borderBottom={false}
								borderColor="gray"
								borderDimColor
								borderLeft={false}
								borderRight={false}
								borderStyle="single"
								borderTop
								key={item.key}
								width="100%"
							/>
						)
					}

					if (item.type === "checkbox") {
						return (
							<Box key={item.key} marginLeft={item.isSubItem ? 2 : 0}>
								<Checkbox
									checked={Boolean(item.value)}
									description={item.description}
									isSelected={isSelected}
									label={item.label}
								/>
							</Box>
						)
					}

					// Action item (button-like, no value display)
					if (item.type === "action") {
						return (
							<Text key={item.key}>
								<Text bold color={isSelected ? COLORS.primaryBlue : undefined}>
									{isSelected ? "❯" : " "}{" "}
								</Text>
								<Text color={isSelected ? COLORS.primaryBlue : "white"}>{item.label}</Text>
								{isSelected && <Text color="gray"> (Enter)</Text>}
							</Text>
						)
					}

					if (item.type === "cycle") {
						return (
							<Text key={item.key}>
								<Text bold color={isSelected ? COLORS.primaryBlue : undefined}>
									{isSelected ? "❯" : " "}{" "}
								</Text>
								<Text color={isSelected ? COLORS.primaryBlue : "white"}>{item.label}: </Text>
								<Text color={COLORS.primaryBlue}>
									{typeof item.value === "string" ? item.value : String(item.value)}
								</Text>
								{isSelected && <Text color="gray"> (Tab to cycle)</Text>}
							</Text>
						)
					}

					// Readonly or editable field
					return (
						<Text key={item.key}>
							<Text bold color={isSelected ? COLORS.primaryBlue : undefined}>
								{isSelected ? "❯" : " "}{" "}
							</Text>
							{item.label && <Text color={isSelected ? COLORS.primaryBlue : "white"}>{item.label}: </Text>}
							<Text color={item.type === "readonly" ? "gray" : COLORS.primaryBlue}>
								{typeof item.value === "string" ? item.value : String(item.value)}
							</Text>
							{item.type === "editable" && isSelected && <Text color="gray"> (Tab to edit)</Text>}
						</Text>
					)
				})}
			</Box>
		)
	}

	// Determine if we're in a subpage (picker, editor, or waiting state)
	const isSubpage =
		isPickingProvider ||
		isPickingModel ||
		isPickingLanguage ||
		isEnteringApiKey ||
		isConfiguringBedrock ||
		isWaitingForCodexAuth ||
		!!codexAuthError ||
		isBedrockCustomFlow ||
		isEditing

	return (
		<Panel currentTab={currentTab} isSubpage={isSubpage} label="Settings" tabs={TABS}>
			{renderContent()}
		</Panel>
	)
}
