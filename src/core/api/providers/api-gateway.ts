import net from "net"
import fs from "node:fs"
import { Logger } from "@/shared/services/Logger"
import { ApiStream, type ApiStreamChunk } from "../transform/stream"
import type { DiracStorageMessage } from "@/shared/messages/content"
import type { DiracTool } from "@/shared/tools"
import type { ApiHandler, ApiHandlerModel } from "../index"
import type { ModelInfo } from "@shared/api"

const SOCKET_PATH = `${process.env.HOME || "/root"}/.dirac/api-gateway.sock`
const DEFAULT_TIMEOUT_MS = 120000

// --- Gateway protocol types (match Go structs in api-gateway/) ---

interface GatewayProviderConfig {
	id: string
	api_key?: string
	base_url?: string
	model?: string
	region?: string
	project_id?: string
	extra?: Record<string, unknown>
}

interface GatewayThinkingConfig {
	type: string
	budget_tokens?: number
}

interface GatewayContentBlock {
	type: string
	text?: string
	tool_use?: {
		id: string
		type: string
		function: { name: string; arguments: string }
	}
	thinking?: string
	signature?: string
	tool_result?: {
		tool_use_id: string
		content: string
		is_error?: boolean
	}
	image_source?: {
		type: string
		mime_type?: string
		data?: string
		url?: string
	}
}

interface GatewayMessage {
	role: string
	content?: string
	content_blocks?: GatewayContentBlock[]
	tool_calls?: Array<{
		id: string
		type: string
		function: { name: string; arguments: string }
	}>
	tool_use_id?: string
	thinking?: string
	name?: string
}

interface GatewayRequest {
	id: number
	stream: boolean
	timeout?: number
	provider: GatewayProviderConfig
	messages: GatewayMessage[]
	system?: string
	tools?: string[]
	max_tokens?: number
	temperature?: number
	top_p?: number
	stop?: string[]
	thinking?: GatewayThinkingConfig
	model_override?: string
}

interface GatewayResponse {
	id: number
	status: number
	body?: GatewayStreamChunk
	error?: { code?: string; message: string; retriable?: boolean }
}

interface GatewayStreamChunk {
	type: string
	index?: number
	text_delta?: string
	json_delta?: string
	tool_call_id?: string
	tool_call_name?: string
	thinking?: string
	usage?: {
		input_tokens: number
		output_tokens: number
		cache_creation_input_tokens?: number
		cache_read_input_tokens?: number
		reasoning_tokens?: number
	}
	finish_reason?: string
	content?: string
	content_blocks?: GatewayContentBlock[]
}

// --- Handler ---

export class ApiGatewayHandler implements ApiHandler {
	private socketPath: string
	private requestId = 0
	private abortController: AbortController | null = null

	constructor(
		private options: {
			providerId: string
			apiKey?: string
			baseUrl?: string
			model?: string
			modelInfo?: ModelInfo
			thinkingBudgetTokens?: number
			enableThinking?: boolean
		},
	) {
		this.socketPath = process.env.DIRAC_API_GATEWAY_SOCKET || SOCKET_PATH
	}

	async *createMessage(
		systemPrompt: string,
		messages: DiracStorageMessage[],
		tools?: DiracTool[],
		_useResponseApi?: boolean,
	): ApiStream {
		this.abortController = new AbortController()
		const reqId = ++this.requestId

		const gatewayMessages = this.serializeMessages(messages)

		const request: GatewayRequest = {
			id: reqId,
			stream: true,
			timeout: DEFAULT_TIMEOUT_MS,
			provider: {
				id: this.options.providerId,
				api_key: this.options.apiKey,
				base_url: this.options.baseUrl,
				model: this.options.model,
			},
			messages: gatewayMessages,
			system: systemPrompt,
			tools: tools?.map((t) => JSON.stringify(t)),
		}

		const modelInfo = this.options.modelInfo
		if (modelInfo?.maxTokens) {
			request.max_tokens = modelInfo.maxTokens
		}
		if (this.options.enableThinking && this.options.thinkingBudgetTokens) {
			request.thinking = {
				type: "enabled",
				budget_tokens: this.options.thinkingBudgetTokens,
			}
		}

		// Content block state tracking for streaming
		const blockTypes = new Map<number, string>() // index → "text" | "thinking" | "tool_use"
		const toolCallAccumulators = new Map<
			number,
			{ id: string; name: string; inputJson: string; callId?: string }
		>()

		const stream = this.connectAndStream(request)

		for await (const response of stream) {
			if (this.abortController?.signal.aborted) return

			if (response.error) {
				const err = new Error(response.error.message)
				;(err as any).status = response.status
				;(err as any).retriable = response.error.retriable
				throw err
			}

			const chunk = response.body
			if (!chunk) continue

			switch (chunk.type) {
				case "content": {
					// content_block_start: track which block type is at this index
					if (chunk.index !== undefined && chunk.content) {
						blockTypes.set(chunk.index, chunk.content)
						if (chunk.content === "tool_use") {
							let acc = toolCallAccumulators.get(chunk.index)
							if (!acc) {
								acc = { id: "", name: "", inputJson: "" }
								toolCallAccumulators.set(chunk.index, acc)
							}
							if (chunk.tool_call_id) acc.id = chunk.tool_call_id
							if (chunk.tool_call_name) acc.name = chunk.tool_call_name
						}
					}
					break
				}
				case "delta": {
					const idx = chunk.index ?? 0
					const blockType = blockTypes.get(idx) ?? "text"

					if (chunk.text_delta) {
						if (blockType === "thinking") {
							yield { type: "reasoning", reasoning: chunk.text_delta } as ApiStreamChunk
						} else {
							yield { type: "text", text: chunk.text_delta } as ApiStreamChunk
						}
					}

					if (chunk.thinking && blockType === "thinking") {
						// signature_delta or thinking_delta
						yield { type: "reasoning", reasoning: chunk.thinking } as ApiStreamChunk
					}

					if (chunk.json_delta || chunk.tool_call_id || chunk.tool_call_name) {
						// Accumulating tool call input JSON
						let acc = toolCallAccumulators.get(idx)
						if (!acc) {
							acc = { id: "", name: "", inputJson: "" }
							toolCallAccumulators.set(idx, acc)
						}
						if (chunk.tool_call_id) acc.id = chunk.tool_call_id
						if (chunk.tool_call_name) acc.name = chunk.tool_call_name
						if (chunk.json_delta) acc.inputJson += chunk.json_delta.replace(/^"|"$/g, "")
					}
					break
				}
				case "stop": {
					// Finalize any pending tool calls
					for (const [idx, acc] of toolCallAccumulators) {
						let args = acc.inputJson
						try {
							args = JSON.stringify(JSON.parse(args))
						} catch {
							// keep raw if unparseable
						}
						yield {
							type: "tool_calls",
							tool_call: {
								call_id: acc.callId || acc.id || undefined,
								function: {
									id: acc.id || undefined,
									name: acc.name || undefined,
									arguments: args || undefined,
								},
							},
						} as ApiStreamChunk
						toolCallAccumulators.delete(idx)
					}

					// Usage chunk
					if (chunk.usage) {
						yield {
							type: "usage",
							inputTokens: chunk.usage.input_tokens,
							outputTokens: chunk.usage.output_tokens,
							cacheWriteTokens: chunk.usage.cache_creation_input_tokens,
							cacheReadTokens: chunk.usage.cache_read_input_tokens,
							reasoningTokens: chunk.usage.reasoning_tokens,
							stopReason: chunk.finish_reason,
						} as ApiStreamChunk
					}
					break
				}
				case "complete": {
					// Stream ended
					return
				}
			}
		}
	}

	getModel(): ApiHandlerModel {
		const modelId = this.options.model || `${this.options.providerId}-default`
		return {
			id: modelId,
			info: (this.options.modelInfo || {
				id: modelId,
				maxTokens: 8192,
				supportsPromptCache: false,
			}) as ModelInfo,
		}
	}

	async abort(): Promise<void> {
		if (this.abortController) {
			this.abortController.abort()
		}
	}

	// --- Message serialization ---

	private serializeMessages(messages: DiracStorageMessage[]): GatewayMessage[] {
		return messages.map((msg) => {
			const gwMsg: GatewayMessage = {
				role: msg.role === "assistant" ? "assistant" : "user",
			}

			if (typeof msg.content === "string") {
				gwMsg.content = msg.content
			} else if (Array.isArray(msg.content)) {
				const blocks: GatewayContentBlock[] = []
				for (const block of msg.content as any[]) {
					switch (block.type) {
						case "text":
							blocks.push({ type: "text", text: block.text })
							break
						case "image":
							if (block.source?.type === "base64") {
								blocks.push({
									type: "image",
									image_source: {
										type: "base64",
										mime_type: block.source.media_type,
										data: block.source.data,
									},
								})
							} else if (block.source?.type === "url") {
								blocks.push({
									type: "image",
									image_source: {
										type: "url",
										url: block.source.url,
									},
								})
							}
							break
						case "tool_use":
							blocks.push({
								type: "tool_use",
								tool_use: {
									id: block.id,
									type: "tool_use",
									function: {
										name: block.name,
										arguments:
											typeof block.input === "string"
												? block.input
												: JSON.stringify(block.input),
									},
								},
							})
							break
						case "tool_result":
							blocks.push({
								type: "tool_result",
								tool_result: {
									tool_use_id: block.tool_use_id,
									content:
										typeof block.content === "string"
											? block.content
											: Array.isArray(block.content)
												? block.content
														.map((c: any) =>
															c.type === "text" ? c.text : "",
														)
														.filter(Boolean)
														.join("\n")
												: "",
									is_error: block.is_error,
								},
							})
							break
						case "thinking":
							blocks.push({
								type: "thinking",
								thinking: block.thinking,
								signature: block.signature,
							})
							break
						case "redacted_thinking":
							blocks.push({
								type: "thinking",
								thinking: "[REDACTED]",
							})
							break
					}
				}
				if (blocks.length > 0) {
					gwMsg.content_blocks = blocks
				}
			}

			// Handle message-level tool calls (OpenAI-style)
			if (msg.role === "assistant" && (msg as any).toolCalls) {
				gwMsg.tool_calls = (msg as any).toolCalls.map((tc: any) => ({
					id: tc.id || tc.function?.id,
					type: "function",
					function: {
						name: tc.function?.name,
						arguments:
							typeof tc.function?.arguments === "string"
								? tc.function.arguments
								: JSON.stringify(tc.function?.arguments),
					},
				}))
			}

			if ((msg as any).toolUseId) {
				gwMsg.tool_use_id = (msg as any).toolUseId
			}

			return gwMsg
		})
	}

	// --- Socket connection ---

	private async *connectAndStream(request: GatewayRequest): AsyncGenerator<GatewayResponse> {
		if (!fs.existsSync(this.socketPath)) {
			throw new Error(
				`API gateway is not running (socket not found at ${this.socketPath}). ` +
				`Ensure the gateway binary is built and the CLI can find it. ` +
				`Set DIRAC_API_GATEWAY_BIN to the binary path if needed.`,
			)
		}

		const socket = await this.connect()
		if (!socket) {
			throw new Error(`Failed to connect to API gateway at ${this.socketPath}`)
		}

		try {
			// Send request
			await new Promise<void>((resolve, reject) => {
				const payload = JSON.stringify(request) + "\n"
				if (!socket.write(payload)) {
					socket.once("drain", () => resolve())
				} else {
					resolve()
				}
				socket.once("error", reject)
			})

			// Read NDJSON responses
			let buffer = ""
			for await (const data of (socket as any as AsyncIterable<Buffer>)) {
				buffer += data.toString()
				const lines = buffer.split("\n")
				buffer = lines.pop() || "" // keep incomplete last line

				for (const line of lines) {
					if (!line.trim()) continue
					try {
						const response: GatewayResponse = JSON.parse(line)
						yield response
						if (response.body?.type === "complete" || response.error) {
							return
						}
					} catch {
						// skip unparseable lines
					}
				}
			}
		} finally {
			socket.destroy()
		}
	}

	private connect(): Promise<net.Socket | null> {
		return new Promise((resolve) => {
			const socket = new net.Socket()
			const timeout = setTimeout(() => {
				socket.destroy()
				resolve(null)
			}, 10000)

			socket.connect(this.socketPath, () => {
				clearTimeout(timeout)
				resolve(socket)
			})

			socket.once("error", (err) => {
				clearTimeout(timeout)
				Logger.warn("ApiGatewayHandler", `Socket error: ${err.message}`)
				resolve(null)
			})
		})
	}
}

export function createApiGatewayHandler(
	providerId: string,
	options: {
		apiKey?: string
		baseUrl?: string
		model?: string
		modelInfo?: ModelInfo
		thinkingBudgetTokens?: number
		enableThinking?: boolean
	},
): ApiHandler {
	return new ApiGatewayHandler({ providerId, ...options })
}
