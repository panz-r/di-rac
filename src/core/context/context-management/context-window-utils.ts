import { ApiHandler } from "@core/api"

/**
 * Gets context window information for the given API handler
 *
 * @param api The API handler to get context window information for
 * @returns An object containing the raw context window size and the effective max allowed size
 */
export function getContextWindowInfo(api: ApiHandler) {
	const HARD_LIMIT = 250_000

	const model = api.getModel()
	let contextWindow = model.info.contextWindow || 128_000
	const maxTokens = model.info.maxTokens || 0

	// Effective input budget: contextWindow minus the output reservation (maxTokens).
	// Floor at 20% of contextWindow so models with very large maxTokens still have input space.
	const inputBudget = maxTokens > 0 ? Math.max(contextWindow - maxTokens, contextWindow * 0.2) : contextWindow

	const maxAllowedSize = Math.min(HARD_LIMIT, Math.max(inputBudget - 40_000, inputBudget * 0.8))

	return { contextWindow, maxAllowedSize }
}
