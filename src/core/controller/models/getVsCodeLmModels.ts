/**
 * VS Code LM Models handler
 * This was part of the VS Code extension integration that has been removed.
 * This stub returns empty models to prevent build errors while maintaining
 * proto compatibility.
 */

import { VsCodeLmModelsArray } from "@shared/proto/dirac/models"
import { EmptyRequest } from "@shared/proto/dirac/common"
import type { Controller } from ".."

/**
 * Handler for getVsCodeLmModels RPC.
 * Returns empty models array since VS Code LM API is not available in CLI mode.
 *
 * @param _request - Empty request (not used)
 * @param _controller - Controller instance (not used)
 * @returns Empty VsCodeLmModelsArray
 */
export async function getVsCodeLmModels(_controller: Controller, _request: EmptyRequest): Promise<VsCodeLmModelsArray> {
	// Return empty models - VS Code LM API is not available in standalone mode
	return VsCodeLmModelsArray.create({ models: [] })
}
