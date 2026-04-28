import type { EmptyRequest } from "@shared/proto/dirac/common"
import { Empty } from "@shared/proto/dirac/common"
import { Logger } from "@/shared/services/Logger"
import type { Controller } from "../index"

export async function openWalkthrough(_controller: Controller, _request: EmptyRequest): Promise<Empty> {
	Logger.log("openWalkthrough: not available in standalone/CLI mode")
	return Empty.create({})
}
