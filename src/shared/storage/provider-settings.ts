import type { StateManager } from "@/core/storage/StateManager"
import type { ProviderSetting } from "@/core/api/providers/api-gateway"

export type SettingScope = "global" | "per-mode"

export function getSettingKey(
	providerId: string,
	mode: "act" | "plan",
	settingKey: string,
	scope: SettingScope,
): string {
	if (scope === "per-mode") {
		return `${providerId}:${mode}:${settingKey}`
	}
	return `${providerId}:global:${settingKey}`
}

export function getSettingsForMode(
	store: Record<string, unknown>,
	providerId: string,
	mode: "act" | "plan",
	schema: ProviderSetting[],
): Record<string, unknown> {
	const result: Record<string, unknown> = {}
	for (const setting of schema) {
		const scope: SettingScope = setting.scope || "global"
		const key = getSettingKey(providerId, mode, setting.key, scope)
		if (key in store) {
			result[setting.key] = store[key]
		} else if (setting.default !== undefined) {
			result[setting.key] = setting.default
		}
	}
	return result
}

export function setProviderSetting(
	stateManager: StateManager,
	providerId: string,
	mode: "act" | "plan",
	settingKey: string,
	scope: SettingScope,
	value: unknown,
): void {
	const store = (stateManager.getGlobalSettingsKey("providerSettings") as Record<string, unknown>) || {}
	const key = getSettingKey(providerId, mode, settingKey, scope)
	store[key] = value
	stateManager.setGlobalState("providerSettings", store)
	stateManager.flushPendingState()
}

export function getProviderSetting(
	stateManager: StateManager,
	providerId: string,
	mode: "act" | "plan",
	settingKey: string,
	scope: SettingScope,
): unknown {
	const store = (stateManager.getGlobalSettingsKey("providerSettings") as Record<string, unknown>) || {}
	const key = getSettingKey(providerId, mode, settingKey, scope)
	return store[key]
}
