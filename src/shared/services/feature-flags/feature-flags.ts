import type { FeatureFlagPayload } from "@/services/feature-flags/providers/IFeatureFlagsProvider"

export enum FeatureFlag {
	// Feature flag for DB-backed welcome banners (What's New modal)
	// When off, hardcoded welcome items are shown instead
	REMOTE_WELCOME_BANNERS = "remote-welcome-banners",
	// Use the websocket mode for OpenAI native Responses API format
	OPENAI_RESPONSES_WEBSOCKET_MODE = "openai-responses-websocket-mode",
}

export const FeatureFlagDefaultValue: Partial<Record<FeatureFlag, FeatureFlagPayload>> = {
	[FeatureFlag.REMOTE_WELCOME_BANNERS]: process.env.E2E_TEST === "true" || process.env.IS_DEV === "true",
	[FeatureFlag.OPENAI_RESPONSES_WEBSOCKET_MODE]: false,
}

export const FEATURE_FLAGS = Object.values(FeatureFlag)
