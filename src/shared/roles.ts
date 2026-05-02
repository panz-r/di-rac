import { buildApiHandler } from "@core/api"
import type { ApiConfiguration, ApiProvider } from "@shared/api"
import { getProviderDefaultModelId, getProviderModelIdKey } from "@shared/providers/provider-registry"

export type ModelRole = "act" | "plan" | "observe"

const ROLE_PROVIDER_KEYS: Record<ModelRole, string> = {
	act: "actModeApiProvider",
	plan: "planModeApiProvider",
	observe: "observerProvider",
}

const ROLE_MODEL_KEYS: Record<ModelRole, string> = {
	act: "actModeApiModelId",
	plan: "planModeApiModelId",
	observe: "observerModelId",
}

export function getRoleProviderKey(role: ModelRole): string {
	return ROLE_PROVIDER_KEYS[role]
}

export function getRoleModelKey(role: ModelRole): string {
	return ROLE_MODEL_KEYS[role]
}

/**
 * Build an ApiHandler for any role. For act/plan, delegates directly to buildApiHandler.
 * For observe (and future roles), clones config, overrides act-mode keys with the
 * role's provider/model, then calls buildApiHandler with "act" mode.
 */
export function buildApiHandlerForRole(config: ApiConfiguration, role: ModelRole) {
	if (role === "act" || role === "plan") {
		return buildApiHandler(config, role)
	}

	const provider = (config as any)[ROLE_PROVIDER_KEYS[role]] as ApiProvider | undefined
	const modelId = (config as any)[ROLE_MODEL_KEYS[role]] as string | undefined

	const effective = { ...config } as Record<string, unknown>
	const targetProvider: ApiProvider = provider || config.actModeApiProvider || "openrouter"
	effective.actModeApiProvider = targetProvider

	const modelKey = getProviderModelIdKey(targetProvider, "act")
	effective[modelKey] = modelId || getProviderDefaultModelId(targetProvider) || ""

	return buildApiHandler(effective as ApiConfiguration, "act")
}
