import { buildApiHandler } from "@/core/api"
import type { ObserverConfig } from "./ObserverConfig"
import { REFLECTOR_SYSTEM_PROMPT } from "./ObserverConfig"
import type { StateManager } from "@core/storage/StateManager"
import type { DiracStorageMessage } from "@shared/messages/content"

export class ReflectorAgent {
	private handler: ReturnType<typeof buildApiHandler> | undefined

	constructor(
		private config: ObserverConfig,
		private stateManager: StateManager,
	) {}

	private getHandler() {
		if (this.handler) return this.handler
		const apiConfig = this.stateManager.getApiConfiguration()
		this.handler = buildApiHandler(apiConfig, "observe")
		return this.handler
	}

	async reflect(observationBlock: string): Promise<string> {
		const handler = this.getHandler()

		const observerMessages: DiracStorageMessage[] = [
			{
				role: "user",
				content: "OBSERVATION LOG TO RESTRUCTURE:\n\n" + observationBlock,
				ts: Date.now(),
			},
		]

		const stream = handler.createMessage(REFLECTOR_SYSTEM_PROMPT, observerMessages, undefined)

		let result = ""
		for await (const chunk of stream) {
			if (chunk.type === "text") {
				result += chunk.text
			}
		}

		return result.trim()
	}

	dispose(): void {
		this.handler = undefined
	}
}
