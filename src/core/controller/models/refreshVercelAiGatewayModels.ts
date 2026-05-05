/**
 * Stub module for Vercel AI Gateway model-related functionality.
 * Model list caching has been removed - api-gateway now handles model fetching.
 * This module is kept as a stub to satisfy existing imports.
 */
import type { ModelInfo } from "@shared/api"
import type { Controller } from ".."

/**
 * Stub refresh function - model caching is removed.
 * Returns an empty record since api-gateway handles model fetching.
 */
export async function refreshVercelAiGatewayModels(_controller: Controller): Promise<Record<string, ModelInfo>> {
	return {}
}
