/**
 * Shared provider metadata utilities
 * Provider list is discovered from the api-gateway (source of truth for
 * which providers are actually implemented). Falls back to the hardcoded
 * PROVIDER_REGISTRY when the gateway is unavailable.
 */

import { useEffect, useMemo, useState } from "react"
import { queryProviderList, type ProviderMeta } from "@/core/api/providers/api-gateway"
import { PROVIDER_LIST } from "@/shared/providers/provider-registry"

// Create a lookup map from provider value to display label
const providerLabels: Record<string, string> = Object.fromEntries(
	PROVIDER_LIST.map((p) => [p.value, p.label]),
)

// Fallback order from registry (used when gateway is unavailable)
const fallbackProviderOrder: string[] = PROVIDER_LIST.map((p) => p.value)

/**
 * Providers that are not supported in CLI.
 * - vscode-lm: Requires VS Code's Language Model API
 */
const CLI_EXCLUDED_PROVIDERS = new Set<string>(["vscode-lm"])

// Module-level cache for gateway-discovered providers
let gatewayProvidersCache: ProviderMeta[] | null = null

/**
 * Get the display label for a provider ID.
 * Uses gateway-discovered label first, falls back to registry label, then title-case.
 */
export function getProviderLabel(providerId: string): string {
	if (gatewayProvidersCache) {
		const meta = gatewayProvidersCache.find((p) => p.id === providerId)
		if (meta?.label) return meta.label
	}
	return providerLabels[providerId] || providerId
}

/**
 * Get the default model ID for a provider from gateway metadata.
 */
export function getProviderDefaultModelId(providerId: string): string {
	if (gatewayProvidersCache) {
		const meta = gatewayProvidersCache.find((p) => p.id === providerId)
		if (meta?.default_model) return meta.default_model
	}
	return ""
}

/**
 * Get the list of valid CLI provider IDs (excluding unsupported providers).
 * Uses cached gateway-discovered list if available, falls back to registry.
 */
export function getValidCliProviders(): string[] {
	if (gatewayProvidersCache) {
		return gatewayProvidersCache
			.map((p) => p.id)
			.filter((p) => !CLI_EXCLUDED_PROVIDERS.has(p))
	}
	return fallbackProviderOrder.filter((p) => !CLI_EXCLUDED_PROVIDERS.has(p))
}

/**
 * Check if a provider ID is valid for CLI use
 */
export function isValidCliProvider(providerId: string): boolean {
	return getValidCliProviders().includes(providerId)
}

export const useValidProviders = () => {
	const [providers, setProviders] = useState<string[]>(() =>
		getValidCliProviders(),
	)

	useEffect(() => {
		if (gatewayProvidersCache) return

		let cancelled = false
		queryProviderList().then((gatewayList) => {
			if (cancelled) return
			if (gatewayList && gatewayList.length > 0) {
				gatewayProvidersCache = gatewayList
				setProviders(
					gatewayList.map((p) => p.id).filter((p) => !CLI_EXCLUDED_PROVIDERS.has(p)),
				)
			}
		})
		return () => {
			cancelled = true
		}
	}, [])

	return useMemo(() => providers, [providers])
}
