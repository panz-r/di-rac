/**
 * Shared provider metadata utilities
 * Provider list is discovered from the api-gateway (source of truth for
 * which providers are actually implemented). Falls back to the hardcoded
 * PROVIDER_REGISTRY when the gateway is unavailable.
 */

import { useEffect, useMemo, useState } from "react"
import { queryProviderList } from "@/core/api/providers/api-gateway"
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
 * - dirac: This is the app itself, not a selectable provider
 */
const CLI_EXCLUDED_PROVIDERS = new Set<string>(["vscode-lm", "dirac"])

// Module-level cache for gateway-discovered providers
let gatewayProvidersCache: string[] | null = null

/**
 * Get the display label for a provider ID
 */
export function getProviderLabel(providerId: string): string {
	return providerLabels[providerId] || providerId
}

/**
 * Get the list of valid CLI provider IDs (excluding unsupported providers).
 * Uses cached gateway-discovered list if available, falls back to registry.
 */
export function getValidCliProviders(): string[] {
	const order = gatewayProvidersCache || fallbackProviderOrder
	return order.filter((p) => !CLI_EXCLUDED_PROVIDERS.has(p))
}

/**
 * Check if a provider ID is valid for CLI use
 */
export function isValidCliProvider(providerId: string): boolean {
	const order = gatewayProvidersCache || fallbackProviderOrder
	return order.includes(providerId) && !CLI_EXCLUDED_PROVIDERS.has(providerId)
}

export const useValidProviders = () => {
	const [providers, setProviders] = useState<string[]>(() => {
		if (gatewayProvidersCache) {
			return gatewayProvidersCache.filter((p) => !CLI_EXCLUDED_PROVIDERS.has(p))
		}
		return fallbackProviderOrder.filter((p) => !CLI_EXCLUDED_PROVIDERS.has(p))
	})

	useEffect(() => {
		if (gatewayProvidersCache) return

		let cancelled = false
		queryProviderList().then((gatewayList) => {
			if (cancelled) return
			if (gatewayList && gatewayList.length > 0) {
				gatewayProvidersCache = gatewayList
				setProviders(gatewayList.filter((p) => !CLI_EXCLUDED_PROVIDERS.has(p)))
			}
		})
		return () => {
			cancelled = true
		}
	}, [])

	return useMemo(() => providers, [providers])
}
