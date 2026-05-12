import { getWorkspaceBasename } from "@core/workspace"
import type { ToggleDiracRuleRequest } from "@shared/types/dirac/file"
import { RuleScope, ToggleDiracRules } from "@shared/types/dirac/file"
import { Logger } from "@/shared/services/Logger"
import type { Controller } from "../index"

/**
 * Toggles a Dirac rule (enable or disable)
 * @param controller The controller instance
 * @param request The toggle request
 * @returns The updated Dirac rule toggles
 */
export async function toggleDiracRule(controller: Controller, request: ToggleDiracRuleRequest): Promise<ToggleDiracRules> {
	const { scope, rulePath, enabled } = request

	if (!rulePath || typeof enabled !== "boolean" || scope === undefined) {
		Logger.error("toggleDiracRule: Missing or invalid parameters", {
			rulePath,
			scope,
			enabled: typeof enabled === "boolean" ? enabled : `Invalid: ${typeof enabled}`,
		})
		throw new Error("Missing or invalid parameters for toggleDiracRule")
	}

	// Handle the three different scopes
	switch (scope) {
		case RuleScope.GLOBAL: {
			const toggles = controller.stateManager.getGlobalSettingsKey("globalDiracRulesToggles")
			toggles[rulePath] = enabled
			controller.stateManager.setGlobalState("globalDiracRulesToggles", toggles)
			break
		}
		case RuleScope.LOCAL: {
			const toggles = controller.stateManager.getWorkspaceStateKey("localDiracRulesToggles")
			toggles[rulePath] = enabled
			controller.stateManager.setWorkspaceState("localDiracRulesToggles", toggles)
			break
		}
		case RuleScope.REMOTE: {
			const toggles = controller.stateManager.getGlobalStateKey("remoteRulesToggles")
			toggles[rulePath] = enabled
			controller.stateManager.setGlobalState("remoteRulesToggles", toggles)
			break
		}
		default:
			throw new Error(`Invalid scope: ${scope}`)
	}

	// Get the current state to return in the response
	const globalToggles = controller.stateManager.getGlobalSettingsKey("globalDiracRulesToggles")
	const localToggles = controller.stateManager.getWorkspaceStateKey("localDiracRulesToggles")
	const remoteToggles = controller.stateManager.getGlobalStateKey("remoteRulesToggles")

	return ToggleDiracRules.create({
		globalDiracRulesToggles: { toggles: globalToggles },
		localDiracRulesToggles: { toggles: localToggles },
		remoteRulesToggles: { toggles: remoteToggles },
	})
}
