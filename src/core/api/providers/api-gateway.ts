import net from "net"
import fs from "node:fs"
import { Logger } from "@/shared/services/Logger"
import { ApiStream, type ApiStreamChunk } from "../transform/stream"
import type { DiracStorageMessage } from "@/shared/messages/content"
import type { DiracTool } from "@/shared/tools"
import type { ApiHandler, ApiHandlerModel } from "../index"
import type { ModelInfo } from "@shared/api"

const SOCKET_PATH = `${process.env.HOME || "/root"}/.dirac/api-gateway.sock`
const DEFAULT_TIMEOUT_MS = 240000

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
	reasoning_effort?: string // "high" or "max" (DeepSeek, OpenAI o-series)
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
	tools?: Record<string, unknown>[]
	max_tokens?: number
	temperature?: number
	top_p?: number
	stop?: string[]
	thinking?: GatewayThinkingConfig
	model_override?: string
	logprobs?: boolean
	top_logprobs?: number
	presence_penalty?: number
	frequency_penalty?: number
	settings?: Record<string, unknown>
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
			reasoningEffort?: string
			settings?: Record<string, unknown>
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
			tools: tools as unknown as Record<string, unknown>[],
		}

		const modelInfo = this.options.modelInfo
		if (modelInfo?.maxTokens) {
			request.max_tokens = modelInfo.maxTokens
		}
		if (this.options.enableThinking && this.options.thinkingBudgetTokens) {
			request.thinking = {
				type: "enabled",
				budget_tokens: this.options.thinkingBudgetTokens,
				reasoning_effort: this.options.reasoningEffort || undefined,
			}
		}

		if (this.options.settings && Object.keys(this.options.settings).length > 0) {
			request.settings = this.options.settings
		}

		// Content block state tracking for streaming
		const blockTypes = new Map<number, string>() // index → "text" | "thinking" | "tool_use"
		const toolCallAccumulators = new Map<
			number,
			{ id: string; name: string; inputJson: string; callId?: string; streamed?: boolean }
		>()

		const stream = this.connectAndStream(request)

		for await (const response of stream) {
			if (this.abortController?.signal.aborted) return

			if (response.error) {
				Logger.error("[Gateway:stream]", `Error from gateway: status=${response.status} code=${response.error.code} msg=${response.error.message} retriable=${response.error.retriable}`)
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
							// Yield immediately so the TUI shows the tool name before arguments stream in
							yield {
								type: "tool_calls",
								tool_call: {
									call_id: acc.id || undefined,
									function: {
										id: acc.id || undefined,
										name: acc.name || undefined,
									},
								},
							} as ApiStreamChunk
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
						// Yield incrementally so the TUI streams tool arguments
						if (acc.name && chunk.json_delta) {
							acc.streamed = true
							yield {
								type: "tool_calls",
								tool_call: {
									call_id: acc.id || undefined,
									function: {
										id: acc.id || undefined,
										name: acc.name,
										arguments: chunk.json_delta,
									},
								},
							} as ApiStreamChunk
						}
					}
					break
				}
				case "stop": {
					// Tool calls were already emitted incrementally during streaming.
					// Finalize any remaining tool calls that didn't get incremental deltas (e.g. tools with no arguments).
					for (const [idx, acc] of toolCallAccumulators) {
						if (acc.streamed) {
							toolCallAccumulators.delete(idx)
							continue
						}
						if (!acc.name) {
							Logger.warn("[Gateway:stream]", `Tool call at index ${idx} has no name (id=${acc.id})`)
							continue
						}
						let args = acc.inputJson
						if (args) {
							try {
								args = JSON.stringify(JSON.parse(args))
							} catch {
								// keep raw if unparseable
							}
						}
						yield {
							type: "tool_calls",
							tool_call: {
								call_id: acc.id || undefined,
								function: {
									id: acc.id || undefined,
									name: acc.name,
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

// --- Provider capabilities types (match Go ProviderInfo) ---

export interface ProviderSetting {
	key: string
	label: string
	type: "toggle" | "slider" | "select" | "text" | "number"
	scope?: "global" | "per-mode"
	default?: unknown
	min?: number
	max?: number
	step?: number
	options?: { value: string; label?: string }[]
	description?: string
	group?: string
	valid_range?: string
}

export interface ProviderFeatures {
	supports_thinking: boolean
	supports_reasoning_effort: boolean
	supports_tools: boolean
	supports_images: boolean
	supports_prompt_cache: boolean
	supports_streaming: boolean
}

export interface ProviderInfo {
	id: string
	default_model: string
	settings?: ProviderSetting[]
	features: ProviderFeatures
}

// --- Gateway query helpers ---

function resolveSocketPath(primary?: string): string | null {
	if (primary && fs.existsSync(primary)) return primary
	if (SOCKET_PATH !== primary && fs.existsSync(SOCKET_PATH)) return SOCKET_PATH
	return null
}

function gatewayQuery<T>(socketPath: string, request: Record<string, unknown>): Promise<T | null> {
	return new Promise((resolve) => {
		const resolved = resolveSocketPath(socketPath)
		if (!resolved) {
			Logger.warn("[Gateway:query]", `No socket found (tried: ${socketPath}, ${SOCKET_PATH})`)
			resolve(null)
			return
		}
		const socket = new net.Socket()
		const timeout = setTimeout(() => {
			socket.destroy()
			resolve(null)
		}, 5000)

		let buffer = ""
		socket.connect(resolved, () => {
			clearTimeout(timeout)
			socket.write(JSON.stringify(request) + "\n")
		})

		socket.on("data", (data) => {
			buffer += data.toString()
			const lines = buffer.split("\n")
			buffer = lines.pop() || ""
			for (const line of lines) {
				if (!line.trim()) continue
				try {
					const resp = JSON.parse(line)
					socket.destroy()
					if (resp.error) {
						Logger.warn("[Gateway:query]", `Error from gateway: ${resp.error.code || "unknown"}: ${resp.error.message}`)
						resolve(null)
					} else if (resp.body) {
						resolve(resp.body as T)
					} else {
						resolve(null)
					}
					return
				} catch {
					// skip
				}
			}
		})

		socket.once("error", (err) => {
			clearTimeout(timeout)
			Logger.warn("[Gateway:query]", `Socket error: ${err.message}`)
			resolve(null)
		})

		socket.once("close", () => {
			clearTimeout(timeout)
			resolve(null)
		})
	})
}

export interface ProviderMeta {
	id: string
	label: string
	default_model?: string
}

export function queryProviderList(socketPath?: string): Promise<ProviderMeta[] | null> {
	const sock = socketPath || process.env.DIRAC_API_GATEWAY_SOCKET || SOCKET_PATH
	return gatewayQuery<{ providers: ProviderMeta[] }>(sock, { type: "list-providers" }).then(
		(r) => r?.providers ?? null,
	)
}

export function queryProviderInfo(providerId: string, socketPath?: string): Promise<ProviderInfo | null> {
	const sock = socketPath || process.env.DIRAC_API_GATEWAY_SOCKET || SOCKET_PATH
	return gatewayQuery<ProviderInfo>(sock, { type: "provider-info", provider: providerId })
}

export interface SettingValidation {
	status: "active" | "inactive"
	value?: unknown
	error?: string
	message?: string
}

export interface ValidateSettingsResult {
	settings: Record<string, SettingValidation>
	errors?: string[]
}

export function queryValidateSettings(
	providerId: string,
	settings: Record<string, unknown>,
	thinking?: { type: string; budget_tokens?: number; reasoning_effort?: string },
	socketPath?: string,
): Promise<ValidateSettingsResult | null> {
	const sock = socketPath || process.env.DIRAC_API_GATEWAY_SOCKET || SOCKET_PATH
	return gatewayQuery<ValidateSettingsResult>(sock, {
		type: "validate-parameters",
		provider: providerId,
		settings,
		thinking,
	})
}

export interface GatewayModelEntry {
	id: string
	name?: string
	description?: string
	context_window?: number
	max_tokens?: number
	supports_images?: boolean
	supports_prompt_cache?: boolean
	supports_thinking?: boolean
	thinking_max_budget?: number
}

export function queryModels(
	providerId: string,
	config?: { api_key?: string; base_url?: string },
	socketPath?: string,
): Promise<GatewayModelEntry[] | null> {
	const sock = socketPath || process.env.DIRAC_API_GATEWAY_SOCKET || SOCKET_PATH
	return gatewayQuery<{ models: GatewayModelEntry[] | null }>(sock, {
		type: "models",
		provider: providerId,
		config: config || {},
	}).then((r) => r?.models ?? null)
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
