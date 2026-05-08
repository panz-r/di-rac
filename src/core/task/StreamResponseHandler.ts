import type { ToolUse } from "@core/assistant-message"
import { JSONParser } from "@streamparser/json"
import { nanoid } from "nanoid"
import {
	DiracAssistantRedactedThinkingBlock,
	DiracAssistantThinkingBlock,
	DiracAssistantToolUseBlock,
	DiracReasoningDetailParam,
} from "@/shared/messages/content"
import { Session } from "@/shared/services/Session"
import { DiracDefaultTool } from "@/shared/tools"

export interface PendingToolUse {
	id: string
	name: string
	inputChunks: string[]
	parsedInput?: unknown
	signature?: string
	jsonParser?: JSONParser
	call_id: string
}

interface ToolUseDeltaBlock {
	id?: string
	type?: string
	name?: string
	input?: string
	signature?: string
}

export interface ReasoningDelta {
	id?: string
	reasoning?: string
	signature?: string
	details?: any[]
	redacted_data?: any
}

export interface PendingReasoning {
	id?: string
	contentChunks: string[]
	signature: string
	redactedThinking: DiracAssistantRedactedThinkingBlock[]
	summary: unknown[] | DiracReasoningDetailParam[]
}

const ESCAPE_MAP: Record<string, string> = {
	"\\n": "\n",
	"\\t": "\t",
	"\\r": "\r",
	'\\"': '"',
	"\\\\": "\\",
}

const ESCAPE_PATTERN = /\\[ntr"\\]/g

export class StreamResponseHandler {
	private toolUseHandler = new ToolUseHandler()
	private reasoningHandler = new ReasoningHandler()

	private _requestId: string | undefined

	public setRequestId(id?: string) {
		if (!this._requestId && id) {
			this._requestId = id
		}
	}

	public get requestId() {
		return this._requestId
	}

	public getHandlers() {
		return {
			toolUseHandler: this.toolUseHandler,
			reasonsHandler: this.reasoningHandler,
		}
	}

	public reset() {
		this._requestId = undefined
		this.toolUseHandler = new ToolUseHandler()
		this.reasoningHandler = new ReasoningHandler()
	}
}

/**
 * Handles streaming native tool use blocks and converts them to DiracAssistantToolUseBlock format
 */
class ToolUseHandler {
	private pendingToolUses = new Map<string, PendingToolUse>()

	processToolUseDelta(delta: ToolUseDeltaBlock, call_id?: string): void {
		if (delta.type !== "tool_use" || !delta.id) {
			return
		}

		let pending = this.pendingToolUses.get(delta.id)
		if (!pending) {
			pending = this.createPendingToolUse(delta.id, delta.name || "", call_id)
		}

		if (delta.name) {
			pending.name = delta.name
		}

		if (delta.signature) {
			pending.signature = delta.signature
		}

		if (delta.input) {
			pending.inputChunks.push(delta.input)
			try {
				pending.jsonParser?.write(delta.input)
			} catch {
				// Expected during streaming - JSONParser may not have complete JSON yet
			}
		}
	}

	getFinalizedToolUse(id: string): DiracAssistantToolUseBlock | undefined {
		const pending = this.pendingToolUses.get(id)
		if (!pending?.name) {
			return undefined
		}

		let input: unknown = {}
		// Always re-parse complete input on finalization — the streaming regex
		// cache (parsedInput) may be truncated if jsonParser.onValue never fired.
		const inputStr = pending.inputChunks.join("")
		if (inputStr) {
			try {
				input = JSON.parse(inputStr)
			} catch {
				// Full parse failed — fall back to cached streaming result
				input = pending.parsedInput ?? this.extractPartialJsonFields(inputStr)
			}
		} else if (pending.parsedInput != null) {
			input = pending.parsedInput
		}

		return {
			type: "tool_use",
			id: pending.id,
			name: pending.name,
			input,
			signature: pending.signature,
			call_id: pending.call_id,
		}
	}

	getAllFinalizedToolUses(summary?: DiracAssistantToolUseBlock["reasoning_details"]): DiracAssistantToolUseBlock[] {
		const results: DiracAssistantToolUseBlock[] = []
		for (const id of this.pendingToolUses.keys()) {
			const toolUse = this.getFinalizedToolUse(id)
			if (toolUse) {
				results.push({ ...toolUse, reasoning_details: summary })
			}
		}
		return results
	}

	hasToolUse(id: string): boolean {
		return this.pendingToolUses.has(id)
	}

	getPartialToolUsesAsContent(): ToolUse[] {
		if (this.pendingToolUses.size === 0) return []

		const results: ToolUse[] = []

		for (const pending of this.pendingToolUses.values()) {
			if (!pending.name) {
				continue
			}

			let input: any = {}
			if (pending.parsedInput != null) {
				input = pending.parsedInput
			} else {
				const inputStr = pending.inputChunks.join("")
				if (inputStr) {
					try {
						input = JSON.parse(inputStr)
					} catch {
						input = this.extractPartialJsonFields(inputStr)
						pending.parsedInput = input
					}
				}
			}

			const params: Record<string, any> = {}
			if (typeof input === "object" && input !== null) {
				for (const [key, value] of Object.entries(input)) {
					params[key] = value
				}
			}
			results.push({
				type: "tool_use",
				name: pending.name as DiracDefaultTool,
				params: params as any,
				partial: true,
				signature: pending.signature,
				isNativeToolCall: true,
				call_id: pending.call_id,
			})
		}

		return results
	}

	reset(): void {
		this.pendingToolUses.clear()
	}

	private createPendingToolUse(id: string, name: string, callId?: string): PendingToolUse {
		const jsonParser = new JSONParser()
		jsonParser.onValue = (info: any) => {
			if (info.stack.length === 0 && info.value && typeof info.value === "object") {
				pending.parsedInput = info.value
			}
		}

		jsonParser.onError = () => {}

		const pending: PendingToolUse = {
			id,
			name,
			inputChunks: [],
			parsedInput: undefined,
			jsonParser,
			call_id: callId || id || nanoid(8),
			signature: undefined,
		}

		this.pendingToolUses.set(id, pending)
		Session.get().updateToolCall(pending.call_id, pending.name)

		return pending
	}

	private extractPartialJsonFields(partialJson: string): Record<string, any> {
		const result: Record<string, any> = {}
		const pattern = /"(\w+)":\s*"((?:[^"\\]|\\.)*)"/g

		for (const match of partialJson.matchAll(pattern)) {
			result[match[1]] = match[2].replace(ESCAPE_PATTERN, (m) => ESCAPE_MAP[m])
		}

		return result
	}
}

/**
 * Handles streaming reasoning content and converts it to the appropriate message format
 */
class ReasoningHandler {
	private pendingReasoning: PendingReasoning | null = null

	processReasoningDelta(delta: ReasoningDelta): void {
		if (!this.pendingReasoning) {
			this.pendingReasoning = {
				id: delta.id,
				contentChunks: [],
				signature: "",
				redactedThinking: [],
				summary: [],
			}
		}

		if (delta.reasoning) {
			this.pendingReasoning.contentChunks.push(delta.reasoning)
		}
		if (delta.signature) {
			this.pendingReasoning.signature = delta.signature
		}
		if (delta.details) {
			if (Array.isArray(delta.details)) {
				this.pendingReasoning.summary.push(...delta.details)
			} else {
				this.pendingReasoning.summary.push(delta.details)
			}
		}
		if (delta.redacted_data) {
			this.pendingReasoning.redactedThinking.push({
				type: "redacted_thinking",
				data: delta.redacted_data,
				call_id: delta.id || this.pendingReasoning.id,
			})
		}
	}

	getCurrentReasoning(): DiracAssistantThinkingBlock | null {
		if (!this.pendingReasoning) {
			return null
		}

		if (!this.pendingReasoning.summary.length && !this.pendingReasoning.contentChunks.length) {
			return null
		}

		if (!this.pendingReasoning.signature && this.pendingReasoning.summary.length) {
			const lastSummary = this.pendingReasoning.summary.at(-1)
			if (lastSummary && typeof lastSummary === "object" && "signature" in lastSummary) {
				if (typeof lastSummary.signature === "string") {
					this.pendingReasoning.signature = lastSummary.signature
				}
			}
		}

		return {
			type: "thinking",
			thinking: this.pendingReasoning.contentChunks.join(""),
			signature: this.pendingReasoning.signature,
			summary: this.pendingReasoning.summary,
			call_id: this.pendingReasoning.id,
		}
	}

	getRedactedThinking(): DiracAssistantRedactedThinkingBlock[] {
		return this.pendingReasoning?.redactedThinking || []
	}

	reset(): void {
		this.pendingReasoning = null
	}
}
