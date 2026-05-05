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
import { queryProviderInfo, queryValidateSettings, type ProviderInfo, type ProviderSetting, type ValidateSettingsResult } from "@/core/api/providers/api-gateway"
import { getProviderSetting, setProviderSetting, type SettingScope } from "@shared/storage/provider-settings"
import type { Controller } from "@/core/controller"
import { StateManager } from "@/core/storage/StateManager"
import { supportsReasoningEffortForModel } from "@/utils/model-utils"
import { version as CLI_VERSION } from "../../../package.json"
import { COLORS } from "../constants/colors"
import { useTerminalSize } from "../hooks/useTerminalSize"
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
import { ROLE_DESCRIPTORS, getRoleStateKey, type ModelRole } from "@/shared/roles"

interface SettingsPanelContentProps {
	onClose: () => void
	controller?: Controller
	initialMode?: "model-picker" | "featured-models"
}

type SettingsTab = "api" | "auto-approve" | "features" | "other"

interface ListItem {
	key: string
	label: string
	type: "checkbox" | "readonly" | "editable" | "separator" | "header" | "spacer" | "action" | "cycle"
	value: string | boolean
	description?: string
	isSubItem?: boolean
	indent?: number
	parentKey?: string
	disabled?: boolean
}

function normalizeReasoningEffort(value: unknown): OpenaiReasoningEffort {
	if (isOpenaiReasoningEffort(value)) {
		return value
	}
	return "low"
}

function nextReasoningEffort(current: OpenaiReasoningEffort, options?: readonly string[]): OpenaiReasoningEffort {
	const opts = options || OPENAI_REASONING_EFFORT_OPTIONS
	const idx = opts.indexOf(current)
	return (opts[(idx + 1) % opts.length] ?? opts[0]) as OpenaiReasoningEffort
}

// --- Generic dynamic settings helpers ---

function buildDynamicItems(
	settings: ProviderSetting[],
	providerId: string,
	mode: "act" | "plan",
	stateManager: StateManager,
	validation?: ValidateSettingsResult | null,
): ListItem[] {
	const items: ListItem[] = []
	let currentGroup = ""

	for (const setting of settings) {
		const scope: SettingScope = setting.scope || "global"
		const key = `dyn:${mode}:${setting.key}`
		const label = setting.label || setting.key
		const v = validation?.settings?.[setting.key]
		const isInactive = v?.status === "inactive"
		// Build description: combine setting description with validation info
		let desc = setting.description || ""
		if (v?.message) desc = v.message
		if (v?.error) desc = (desc ? desc + " | " : "") + "Error: " + v.error
		if (setting.valid_range) desc = (desc ? desc + " " : "") + `(${setting.valid_range})`

		// Group header (indented under the role)
		if (setting.group && setting.group !== currentGroup) {
			currentGroup = setting.group
			items.push({
				key: `dynGroup:${setting.group}`,
				label: setting.group.charAt(0).toUpperCase() + setting.group.slice(1),
				type: "header",
				value: "",
				indent: 2,
			})
		}

		switch (setting.type) {
			case "select": {
				const opts = setting.options || []
				const val = getProviderSetting(stateManager, providerId, mode, setting.key, scope)
				const currentOpt = opts.find(o => o.value === val) || opts[0]
				items.push({
					key,
					label,
					type: "cycle",
					value: currentOpt?.label || currentOpt?.value || "",
					description: desc || undefined,
					indent: 4,
					disabled: isInactive,
				})
				break
			}
			case "toggle": {
				const val = getProviderSetting(stateManager, providerId, mode, setting.key, scope)
				items.push({
					key,
					label,
					type: "checkbox",
					value: val !== undefined ? !!val : !!setting.default,
					description: desc || undefined,
					indent: 4,
					disabled: isInactive,
				})
				break
			}
			case "slider": {
				const min = setting.min ?? 0
				const val = getProviderSetting(stateManager, providerId, mode, setting.key, scope)
				const numVal = typeof val === "number" ? val : (typeof setting.default === "number" ? setting.default : min)
				items.push({
					key,
					label,
					type: "editable",
					value: String(numVal),
					description: desc || undefined,
					indent: 4,
					disabled: isInactive,
				})
				break
			}
			case "text": {
				const val = getProviderSetting(stateManager, providerId, mode, setting.key, scope)
				items.push({
					key,
					label,
					type: "editable",
					value: typeof val === "string" ? val : (typeof setting.default === "string" ? setting.default : ""),
					description: desc || undefined,
					indent: 4,
					disabled: isInactive,
				})
				break
			}
			case "number": {
				const val = getProviderSetting(stateManager, providerId, mode, setting.key, scope)
				const numVal = typeof val === "number" ? val : (typeof setting.default === "number" ? setting.default : 0)
				items.push({
					key,
					label,
					type: "editable",
					value: String(numVal),
					description: desc || undefined,
					indent: 4,
					disabled: isInactive,
				})
				break
			}
		}
	}
	return items
}


function computeNextValue(
	currentVal: unknown,
	setting: ProviderSetting,
): unknown {
	switch (setting.type) {
		case "select": {
			const opts = setting.options || []
			const currentStr = currentVal != null ? String(currentVal) : ""
			const idx = opts.findIndex(o => o.value === currentStr)
			const nextIdx = (idx + 1) % opts.length
			return opts[nextIdx]?.value ?? ""
		}
		case "toggle":
			return !currentVal
		case "slider": {
			const min = setting.min ?? 0
			const max = setting.max ?? 100
			const step = setting.step ?? 1
			const current = typeof currentVal === "number" ? currentVal : min
			const next = current + step
			return next > max + 0.001 ? min : next
		}
		case "text":
			return currentVal
		case "number":
			return currentVal
	}
	return currentVal
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
}) => {
	const { isRawModeSupported } = useStdinContext()
	const stateManager = StateManager.get()

	// UI state
	const [currentTab, setCurrentTab] = useState<SettingsTab>("api")
	const [selectedIndex, setSelectedIndex] = useState(0)
	const [isEditing, setIsEditing] = useState(false)
	const [isPickingModel, setIsPickingModel] = useState(initialMode === "model-picker")
	const [pickingModelKey, setPickingModelKey] = useState<"actModelId" | "planModelId" | null>(
		initialMode ? "actModelId" as const : null,
	)
	const [isPickingProvider, setIsPickingProvider] = useState(false)
	const [isPickingLanguage, setIsPickingLanguage] = useState(false)
	const [isEnteringApiKey, setIsEnteringApiKey] = useState(false)
	const [pendingProvider, setPendingProvider] = useState<string | null>(null)
	const [isConfiguringBedrock, setIsConfiguringBedrock] = useState(false)
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

		// Gateway-discovered provider capabilities (cached per provider)
		const [providerInfoCache, setProviderInfoCache] = useState<Record<string, ProviderInfo>>({})
		useEffect(() => {
			if (!provider || provider in providerInfoCache) return
			queryProviderInfo(provider).then((info) => {
				if (info) setProviderInfoCache((prev) => ({ ...prev, [provider]: info }))
			}).catch(() => {})
		}, [provider])

			// Validation results for dynamic settings (per provider+role)
			const [validationCache, setValidationCache] = useState<Record<string, ValidateSettingsResult>>({})
			useEffect(() => {
				const info = providerInfoCache[provider]
				if (!info?.settings?.length) return
				const settingsStore = (stateManager.getGlobalSettingsKey("providerSettings") || {}) as Record<string, unknown>
				// Query validation for each role
				for (const desc of ROLE_DESCRIPTORS) {
					const role = desc.role as "act" | "plan"
					const settings: Record<string, unknown> = {}
					for (const s of info.settings) {
						const scope: SettingScope = s.scope || "global"
						const modeKey = scope === "per-mode" ? role : "global"
						const key = `${provider}:${modeKey}:${s.key}`
						if (key in settingsStore) settings[s.key] = settingsStore[key]
						else if (s.default !== undefined) settings[s.key] = s.default
					}
					const budgetKey = getRoleStateKey(desc.role, "thinkingBudgetTokens")
					const budget = (stateManager as any).getGlobalSettingsKey(budgetKey) as number
					const thinking = budget > 0 ? { type: "enabled", budget_tokens: budget } : undefined
					queryValidateSettings(provider, settings, thinking).then((result) => {
						if (result) {
							setValidationCache((prev) => ({ ...prev, [provider + ":" + desc.role]: result }))
							for (const [key, validation] of Object.entries(result.settings)) {
								if (validation.value !== undefined) {
									const setting = info.settings?.find(s => s.key === key)
									if (setting) {
										const scope: SettingScope = setting.scope || "global"
										setProviderSetting(stateManager, provider, role, key, scope, validation.value)
									}
								}
							}
						}
					}).catch(() => {})
				}
			}, [provider, providerInfoCache, actThinkingEnabled, planThinkingEnabled])

	// Terminal size for virtual scrolling
	const { rows: terminalRows } = useTerminalSize()

	// Role-based picking state
	const [configuringRole, setConfiguringRole] = useState<ModelRole | null>(null)
	const [pickingModelRole, setPickingModelRole] = useState<ModelRole | null>(null)
	// Read model IDs from state (re-reads when refreshKey changes)

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
			// Legacy flags for providers without gateway-declared settings
			const providerUsesReasoningEffort = provider === "openai-native"
			const showActReasoningEffort = supportsReasoningEffortForModel(actModelId || "")
			const showPlanReasoningEffort = supportsReasoningEffortForModel(planModelId || "")
			const showActThinkingOption = !providerUsesReasoningEffort && !showActReasoningEffort
			const showPlanThinkingOption = !providerUsesReasoningEffort && !showPlanReasoningEffort

		switch (currentTab) {
				case "api":
					return ROLE_DESCRIPTORS.flatMap((desc): ListItem[] => {
						const roleProvider = (stateManager as any).getGlobalSettingsKey(desc.providerKey) as string | undefined
						const actProvider = stateManager.getApiConfiguration().actModeApiProvider || ""
						const inheritsFromAct = desc.providerInheritsFromAct && !roleProvider
						const isEnabled = desc.enabledKey ? (stateManager as any).getGlobalSettingsKey(desc.enabledKey) : true
						const effectiveProvider = roleProvider || (inheritsFromAct ? actProvider : "")
						const roleItems: ListItem[] = []

						roleItems.push({ key: `${desc.role}Spacer`, label: "", type: "spacer", value: "" })
						roleItems.push({ key: `${desc.role}Header`, label: desc.label, type: "header", value: "" })

						if (desc.enabledKey) {
							roleItems.push({ key: desc.enabledKey, label: "Enabled", type: "checkbox", value: !!isEnabled })
						}
						if (isEnabled) {
							roleItems.push({
								key: desc.providerKey,
								label: "Provider",
								type: "editable",
								value: inheritsFromAct
									? `${getProviderLabel(actProvider)} (same as Act)`
									: roleProvider ? getProviderLabel(roleProvider) : "not configured",
							})
							if (effectiveProvider) {
								const modelIdKey = getProviderModelIdKey(effectiveProvider as ApiProvider, desc.role)
								const modelId = (stateManager as any).getGlobalSettingsKey(modelIdKey) as string
								roleItems.push({
									key: modelIdKey,
									label: "Model ID",
									type: "editable",
									value: modelId || "not set",
								})
																// Provider settings for act/plan roles
								if (desc.role === "act" || desc.role === "plan") {
									const mode = desc.role as "act" | "plan"
									const roleProviderInfo = providerInfoCache[effectiveProvider]
									if (roleProviderInfo?.settings?.length) {
									// Dynamic: render settings from gateway-discovered schema
									if (roleProviderInfo.features?.supports_thinking) {
										const isAct = mode === "act"
										const thinkingEnabled = isAct ? actThinkingEnabled : planThinkingEnabled
										roleItems.push({
											key: isAct ? "actThinkingEnabled" : "planThinkingEnabled",
											label: "Enable thinking",
											type: "checkbox",
											value: thinkingEnabled,
										})
									}
									roleItems.push(...buildDynamicItems(roleProviderInfo.settings, effectiveProvider, mode, stateManager, validationCache[effectiveProvider + ":" + desc.role]))
									} else {
										// Legacy: hardcoded thinking/reasoning for providers without gateway settings
										const isAct = mode === "act"
										const showThinking = isAct ? showActThinkingOption : showPlanThinkingOption
										const showReasoning = isAct ? showActReasoningEffort : showPlanReasoningEffort
										const thinkingEnabled = isAct ? actThinkingEnabled : planThinkingEnabled
										const reasoningVal = isAct ? actReasoningEffort : planReasoningEffort
										if (showThinking) {
											roleItems.push({
												key: isAct ? "actThinkingEnabled" : "planThinkingEnabled",
												label: "Enable thinking",
												type: "checkbox",
												value: thinkingEnabled,
											})
										}
										if (showReasoning) {
											roleItems.push({
												key: isAct ? "actReasoningEffort" : "planReasoningEffort",
												label: "Reasoning effort",
												type: "cycle",
												value: reasoningVal,
											})
										}
									}
								}
							}
						}
						return roleItems
					})

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
			actThinkingEnabled,
		planThinkingEnabled,
			actReasoningEffort,
		planReasoningEffort,
		autoApproveSettings,
		features,
		preferredLanguage,
		telemetry,
		providerInfoCache,
		validationCache,
	])

	// Reset selection when changing tabs
	const handleTabChange = useCallback((tabKey: string) => {
		setCurrentTab(tabKey as SettingsTab)
		setSelectedIndex(0)
		setIsEditing(false)
		setIsPickingModel(false)
		setPickingModelKey(null)
		setIsPickingProvider(false)
		setConfiguringRole(null)
		setPickingModelRole(null)
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
			} else {
				setPlanReasoningEffort(effort)
				stateManager.setGlobalState("planModeReasoningEffort", effort)
			}
			rebuildTaskApi()
		},
		[rebuildTaskApi, stateManager],
	)

	// Handle toggle/edit for selected item
	// source: "tab" or "enter" — Tab opens model list, Enter opens text editor for model fields
	const handleAction = useCallback((source?: "tab" | "enter") => {
		const item = items[selectedIndex]
		if (!item || item.type === "readonly" || item.type === "separator" || item.type === "header" || item.type === "spacer" || item.disabled)
			return

		if (item.type === "action") {
			return
		}

		// Dynamic settings handler (dyn: prefix)
		if ((item.type === "cycle" || item.type === "checkbox") && item.key.startsWith("dyn:")) {
			const parts = item.key.split(":")
			const mode = parts[1] as "act" | "plan"
			const settingKey = parts.slice(2).join(":")
			const setting = providerInfoCache[provider]?.settings?.find(s => s.key === settingKey)
			if (setting) {
				const scope: SettingScope = setting.scope || "global"
				const currentVal = getProviderSetting(stateManager, provider, mode, setting.key, scope)
				const nextVal = computeNextValue(currentVal, setting)
				setProviderSetting(stateManager, provider, mode, setting.key, scope, nextVal)
				refreshModelIds()
				rebuildTaskApi()
			}
			return
		}

		if (item.type === "cycle") {
			const targetMode = item.key === "actReasoningEffort" ? "act" : item.key === "planReasoningEffort" ? "plan" : undefined
			if (targetMode) {
				const currentEffort = targetMode === "act" ? actReasoningEffort : planReasoningEffort
				setReasoningEffortForMode(targetMode, nextReasoningEffort(currentEffort))
			}
			return
		}

		if (item.type === "editable") {
			// Provider fields — open provider picker
			const providerDesc = item.key === "provider"
				? ROLE_DESCRIPTORS.find((d) => d.role === "act")
				: ROLE_DESCRIPTORS.find((d) => d.providerKey === item.key)
			if (providerDesc) {
				setConfiguringRole(providerDesc.role)
				setIsPickingProvider(true)
				return
			}

			// Model ID fields — open model picker or inline editor
			const isModelField = item.key.endsWith("ModelId") || item.key === "observerModelId"
			if (isModelField) {
				const modelRole = item.key.startsWith("actMode") ? "act" as ModelRole
					: item.key.startsWith("planMode") ? "plan" as ModelRole
					: "observe" as ModelRole
				const roleDesc = ROLE_DESCRIPTORS.find((d) => d.role === modelRole)
				if (roleDesc) {
					const roleProvider = (stateManager as any).getGlobalSettingsKey(roleDesc.providerKey) as string
					const actProvider = stateManager.getApiConfiguration().actModeApiProvider || ""
					const effectiveProvider = roleProvider || (roleDesc.providerInheritsFromAct ? actProvider : "")
					if (source !== "enter" && effectiveProvider && hasModelPicker(effectiveProvider)) {
						setPickingModelRole(modelRole)
						setIsPickingModel(true)
						return
					}
				}
				// Enter or no model list — fall through to inline edit
			}

			// Language field
			if (item.key === "language") {
				setIsPickingLanguage(true)
				return
			}

			// Fallback: inline text edit
			setEditValue(typeof item.value === "string" ? item.value : "")
			setIsEditing(true)
			return
		}

		// Checkbox handling
		const newValue = !item.value

		// Role enable toggles (observer, future roles)
		const roleWithEnable = ROLE_DESCRIPTORS.find((d) => d.enabledKey && d.enabledKey === item.key)
		if (roleWithEnable?.enabledKey) {
			const roleNewVal = !item.value;
			(stateManager as any).setGlobalState(roleWithEnable.enabledKey, roleNewVal)
			stateManager.flushPendingState()
			refreshModelIds()
			if (roleWithEnable.role === "observe") {
				controller?.task?.toggleObserver(roleNewVal)
			}
			return
		}

		// Feature settings (simple toggles)
		if (item.key in FEATURE_SETTINGS) {
			toggleFeature(item.key as FeatureKey)
			return
		}


		// Thinking toggles - set budget to 1024 when enabled, 0 when disabled
		if (item.key === "actThinkingEnabled") {
			setActThinkingEnabled(newValue)
			stateManager.setGlobalState("actModeThinkingBudgetTokens", newValue ? 1024 : 0)
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
				awsAuthentication: "credentials",
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
			// Role model selection (observer, future roles)
			if (pickingModelRole) {
				const desc = ROLE_DESCRIPTORS.find((d) => d.role === pickingModelRole)
				if (desc) {
					const roleProvider = (stateManager as any).getGlobalSettingsKey(desc.providerKey) as string
					const actProv = stateManager.getApiConfiguration().actModeApiProvider || ""
					const effectiveProvider = roleProvider || (desc.providerInheritsFromAct ? actProv : "")
					if (effectiveProvider) {
						const modelKey = getProviderModelIdKey(effectiveProvider as ApiProvider, desc.role)
						stateManager.setGlobalState(modelKey, modelId)
					}
					await stateManager.flushPendingState()
					if (desc.role === "observe") {
						controller?.task?.toggleObserver(false)
						controller?.task?.toggleObserver(true)
					}
				}
				setPickingModelRole(null)
				setIsPickingModel(false)
				refreshModelIds()
				rebuildTaskApi()
				if (initialMode) onClose()
				return
			}

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
			const providerForSelection = pickingModelKey === "actModelId"
				? actProvider || planProvider
				: planProvider || actProvider
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

			const stateKey = pickingModelKey === "actModelId" ? actKey : planKey
			if (stateKey) stateManager.setGlobalState(stateKey, modelId)
			if (modelInfo) {
				const infoKey =
					pickingModelKey === "actModelId" ? "actModeOpenRouterModelInfo" : "planModeOpenRouterModelInfo"
				stateManager.setGlobalState(infoKey, modelInfo)
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
		[pickingModelKey, pickingModelRole, stateManager, controller, provider, refreshModelIds, initialMode, onClose],
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

	const handleProviderSelect = useCallback(
		async (providerId: string) => {
			const role = configuringRole || undefined

			// Special handling for Dirac - uses OAuth
			if (providerId === "dirac") {
				setIsPickingProvider(false)
				setConfiguringRole(null)
				await applyProviderConfig({ providerId: "dirac", role, controller })
				setProvider("dirac")
				refreshModelIds()
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
				const apiConfig = stateManager.getApiConfiguration()
				const fieldName = Array.isArray(keyField) ? keyField[0] : keyField
				const existingKey = (apiConfig as Record<string, string>)[fieldName] || ""
				if (existingKey) {
					// Key already configured — just apply provider for this role
					await applyProviderConfig({ providerId, role, controller })
					setProvider(providerId)
					refreshModelIds()
					setIsPickingProvider(false)
					setConfiguringRole(null)
				} else {
					// Need to enter API key
					setPendingProvider(providerId)
					setApiKeyValue("")
					setIsPickingProvider(false)
					setIsEnteringApiKey(true)
				}
			} else {
				await applyProviderConfig({ providerId, role, controller })
				setProvider(providerId)
				refreshModelIds()
				setIsPickingProvider(false)
				setConfiguringRole(null)
			}
		},
		[stateManager, controller, refreshModelIds, configuringRole],
		)

		// Handle Tab-to-edit on a configured provider in the picker
		const handleProviderEdit = useCallback(
			(providerId: string) => {
				setIsPickingProvider(false)
				setPendingProvider(providerId)
				const keyField = ProviderToApiKeyMap[providerId as ApiProvider]
				if (keyField) {
					const apiConfig = stateManager.getApiConfiguration()
					const fieldName = Array.isArray(keyField) ? keyField[0] : keyField
					const existingKey = (apiConfig as Record<string, string>)[fieldName] || ""
					setApiKeyValue(existingKey)
					setIsEnteringApiKey(true)
				}
			},
			[stateManager],
		)

	// Handle API key submission after provider selection
	const handleApiKeySubmit = useCallback(
		async (submittedValue: string) => {
			if (!pendingProvider || !submittedValue.trim()) {
				return
			}

			const role = configuringRole || undefined
			await applyProviderConfig({ providerId: pendingProvider, apiKey: submittedValue.trim(), role, controller })
			setProvider(pendingProvider)
			refreshModelIds()
			setIsEnteringApiKey(false)
			setPendingProvider(null)
			setApiKeyValue("")
			setConfiguringRole(null)
		},
		[pendingProvider, controller, refreshModelIds, configuringRole],
	)

	// Handle Bedrock configuration complete
	const handleBedrockComplete = useCallback(
		(bedrockConfig: BedrockConfig) => {
			const role = configuringRole || undefined
			setProvider("bedrock")
			refreshModelIds()
			setIsConfiguringBedrock(false)
			setPendingProvider(null)
			setConfiguringRole(null)
			applyBedrockConfig({ bedrockConfig, role, controller })
		},
		[controller, refreshModelIds, configuringRole],
	)

	// Handle saving edited value
	const handleSave = useCallback(() => {
		const item = items[selectedIndex]
		if (!item || item.disabled) return

		// Dynamic settings editable handler
		if (item.key.startsWith("dyn:")) {
			const parts = item.key.split(":")
			const mode = parts[1] as "act" | "plan"
			const settingKey = parts.slice(2).join(":")
			const setting = providerInfoCache[provider]?.settings?.find(s => s.key === settingKey)
			if (setting) {
				const scope: SettingScope = setting.scope || "global"
				let val: unknown = editValue
				if (setting.type === "slider") {
					const num = parseFloat(editValue)
					if (isNaN(num)) { setIsEditing(false); return }
					const min = setting.min ?? -Infinity
					const max = setting.max ?? Infinity
					val = Math.min(max, Math.max(min, num))
				}
				setProviderSetting(stateManager, provider, mode, setting.key, scope, val)
				refreshModelIds()
				rebuildTaskApi()
			}
			setIsEditing(false)
			return
		}

		// Provider-specific model ID keys (e.g., actModeNvidiaNimModelId, actModeApiModelId)
			if (item.key.endsWith("ModelId") || item.key === "observerModelId") {
				stateManager.setGlobalState(item.key as any, editValue || undefined)
				stateManager.flushPendingState()
				refreshModelIds()
				rebuildTaskApi()
			} else if (item.key === "language") {
				setPreferredLanguage(editValue)
				stateManager.setGlobalState("preferredLanguage", editValue)
			}
		setIsEditing(false)
	}, [items, selectedIndex, editValue, stateManager, refreshModelIds, rebuildTaskApi])

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
			if (isPickingProvider || configuringRole) {
				if (key.escape) {
					setIsPickingProvider(false)
					setConfiguringRole(null)
				}
				return
			}


			// Model picker mode - escape to close, input is handled by ModelPicker
			if (isPickingModel) {
				if (key.escape) {
					setIsPickingModel(false)
					setPickingModelKey(null)
					setPickingModelRole(null)
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
			if (key.tab) {
				handleAction("tab")
				return
			}
			if (key.return) {
				handleAction("enter")
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
						<ProviderPicker isActive={isPickingProvider} onSelect={handleProviderSelect} onEdit={handleProviderEdit} />
					</Box>
					<Box marginTop={1}>
						<Text color="gray">Type to search, arrows to navigate, Enter to select, Tab to edit, Esc to cancel</Text>
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
						setConfiguringRole(null)
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
						setConfiguringRole(null)
					}}
					onComplete={handleBedrockComplete}
				/>
			)
		}


		if (isPickingModel && (pickingModelKey || pickingModelRole)) {
			let label: string
			let pickerProvider: string
			if (pickingModelRole) {
				const desc = ROLE_DESCRIPTORS.find((d) => d.role === pickingModelRole)
				const roleProvider = desc ? (stateManager as any).getGlobalSettingsKey(desc.providerKey) as string : ""
				const actProv = stateManager.getApiConfiguration().actModeApiProvider || ""
				pickerProvider = roleProvider || (desc?.providerInheritsFromAct ? actProv : "")
				label = `Model ID (${desc?.label || pickingModelRole})`
			} else {
				label = pickingModelKey === "actModelId" ? "Model ID (Act)" : "Model ID (Plan)"
				pickerProvider = provider
			}
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
							provider={pickerProvider}
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
					{item?.description && <Text color="gray">{item.description}</Text>}
					<Box marginTop={1}>
						<Text color="white">{editValue}</Text>
						<Text color="gray">|</Text>
					</Box>
					<Text color="gray">Enter to save, Esc to cancel</Text>
				</Box>
			)
		}

			// Virtual scrolling: only render visible items around selectedIndex
			const panelOverhead = 10
			const visibleRows = Math.max(terminalRows - panelOverhead, 6)
			const halfWindow = Math.floor(visibleRows / 2)
			let scrollStart = Math.max(0, selectedIndex - halfWindow)
			let scrollEnd = Math.min(items.length, scrollStart + visibleRows)
			if (scrollEnd - scrollStart < visibleRows) {
				scrollStart = Math.max(0, scrollEnd - visibleRows)
			}
			const visibleItems = items.slice(scrollStart, scrollEnd)

		return (
			<Box flexDirection="column">
				{scrollStart > 0 && <Text color="gray">{`  ↑ ${scrollStart} more above`}</Text>}
				{visibleItems.map((item, i) => {
					const idx = scrollStart + i
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
							<Box key={item.key} marginLeft={item.indent ?? (item.isSubItem ? 2 : 0)}>
								<Checkbox
									checked={Boolean(item.value)}
									description={item.description}
									isSelected={isSelected}
									disabled={item.disabled}
									label={item.disabled ? item.label + " (inactive)" : item.label}
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
							<Box key={item.key} marginLeft={item.indent ?? (item.isSubItem ? 2 : 0)}>
							<Text>
								<Text bold color={isSelected ? COLORS.primaryBlue : undefined}>
									{isSelected ? "❯" : " "}{" "}
								</Text>
								<Text color={item.disabled ? "gray" : (isSelected ? COLORS.primaryBlue : "white")}>{`${item.label}${item.disabled ? " (inactive)" : ""}: `}</Text>
								<Text color={item.disabled ? "gray" : COLORS.primaryBlue}>
									{typeof item.value === "string" ? item.value : String(item.value)}
								</Text>
								{isSelected && !item.disabled && <Text color="gray"> (Tab to cycle)</Text>}
							</Text>
							</Box>
						)
					}

					// Readonly or editable field
					return (
						<Box key={item.key} marginLeft={item.indent ?? (item.isSubItem ? 2 : 0)}>
							<Text>
								<Text bold color={isSelected ? COLORS.primaryBlue : undefined}>
									{isSelected ? "❯" : " "}{" "}
								</Text>
								{item.label && <Text color={item.disabled ? "gray" : (isSelected ? COLORS.primaryBlue : "white")}>{`${item.label}${item.disabled ? " (inactive)" : ""}: `}</Text>}
								<Text color={item.disabled ? "gray" : (item.type === "readonly" ? "gray" : COLORS.primaryBlue)}>
									{typeof item.value === "string" ? item.value : String(item.value)}
								</Text>
								{item.type === "editable" && isSelected && !item.disabled && <Text color="gray"> (Tab to edit)</Text>}
							</Text>
						</Box>
					)
				})}
				{scrollEnd < items.length && <Text color="gray">{`  ↓ ${items.length - scrollEnd} more below`}</Text>}
			</Box>
		)
	}

	// Determine if we're in a subpage (picker, editor, or waiting state)
	const isSubpage =
		isPickingProvider ||
		!!configuringRole ||
		isPickingModel ||
		isPickingLanguage ||
		isEnteringApiKey ||
		isConfiguringBedrock ||
		isBedrockCustomFlow ||
		isEditing

	return (
		<Panel currentTab={currentTab} isSubpage={isSubpage} label="Settings" tabs={TABS}>
			{renderContent()}
		</Panel>
	)
}
