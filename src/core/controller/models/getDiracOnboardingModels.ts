import { OnboardingModelGroup } from "@/shared/proto/dirac/state"

// Onboarding models feature is not available in CLI/TUI
export function getDiracOnboardingModels(): OnboardingModelGroup {
	return { models: [] }
}

export function clearOnboardingModelsCache(): void {
	// no-op
}