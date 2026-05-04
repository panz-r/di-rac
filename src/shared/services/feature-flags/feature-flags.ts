import type { FeatureFlagPayload } from "@/services/feature-flags/providers/IFeatureFlagsProvider"

export enum FeatureFlag {
	// Feature flag for DB-backed welcome banners (What's New modal)
	// When off, hardcoded welcome items are shown instead
	REMOTE_WELCOME_BANNERS = "remote-welcome-banners",
	// Feature flag for upstream Dirac recommended model cards
	DIRAC_RECOMMENDED_MODELS_UPSTREAM = "dirac-recommended-models-upstream",
	// Rollout flag for Dirac provider model sourcing:
	// off => OpenRouter model list, on => Dirac endpoint model list.
	EXTENSION_DIRAC_MODELS_ENDPOINT = "extension_dirac_models_endpoint",
	// Use the websocket mode for OpenAI native Responses API format
	OPENAI_RESPONSES_WEBSOCKET_MODE = "openai-responses-websocket-mode",
}

export const FeatureFlagDefaultValue: Partial<Record<FeatureFlag, FeatureFlagPayload>> = {
	[FeatureFlag.REMOTE_WELCOME_BANNERS]: process.env.E2E_TEST === "true" || process.env.IS_DEV === "true",
	[FeatureFlag.DIRAC_RECOMMENDED_MODELS_UPSTREAM]: false,
	[FeatureFlag.EXTENSION_DIRAC_MODELS_ENDPOINT]: false,
	[FeatureFlag.OPENAI_RESPONSES_WEBSOCKET_MODE]: false,
}

export const FEATURE_FLAGS = Object.values(FeatureFlag)
