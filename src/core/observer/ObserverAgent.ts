import { buildApiHandler } from "@/core/api"
import type { ObserverConfig, ObservationType } from "./ObserverConfig"
import { 
    OBSERVER_SUMMARIZER_PROMPT, 
    OBSERVER_WATCHER_PROMPT, 
    OBSERVER_FILTER_PROMPT 
} from "./ObserverConfig"
import type { StateManager } from "@core/storage/StateManager"
import { formatContentBlockToMarkdown } from "@integrations/misc/export-markdown"
import type { DiracStorageMessage } from "@shared/messages/content"

export class ObserverAgent {
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

	serializeMessages(messages: DiracStorageMessage[]): string {
		return messages
			.map((msg) => {
				const role = msg.role === "user" ? "USER" : "ASSISTANT"
				const content =
					typeof msg.content === "string"
						? msg.content
						: Array.isArray(msg.content)
							? (msg.content as any[]).map((b) => formatContentBlockToMarkdown(b)).join("\n")
							: String(msg.content)
				return `[${role}]: ${content}`
			})
			.join("\n\n")
	}

	/**
	 * Perform a specialized observation task.
	 */
	async observe(messages: DiracStorageMessage[], type: ObservationType): Promise<string> {
		const handler = this.getHandler()
		const serialized = this.serializeMessages(messages)

		let systemPrompt = OBSERVER_SUMMARIZER_PROMPT
		if (type === "watcher") systemPrompt = OBSERVER_WATCHER_PROMPT
		else if (type === "filter") systemPrompt = OBSERVER_FILTER_PROMPT

		const observerMessages: DiracStorageMessage[] = [
			{ role: "user", content: serialized, ts: Date.now() },
		]

		const stream = handler.createMessage(systemPrompt, observerMessages, undefined)

		let result = ""
		for await (const chunk of stream) {
			if (chunk.type === "text") {
				result += chunk.text
			}
		}

		return result.trim()
	}

	/** Legacy alias for backward compatibility */
	async compress(messages: DiracStorageMessage[]): Promise<string> {
		return this.observe(messages, "summary")
	}

	dispose(): void {
		this.handler = undefined
	}
}
