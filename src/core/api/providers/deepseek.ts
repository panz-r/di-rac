import { DeepSeekModelId, deepSeekDefaultModelId, deepSeekModels, ModelInfo } from "@shared/api"
import { calculateApiCostOpenAI } from "@utils/cost"
import OpenAI from "openai"
import type { ChatCompletionTool as OpenAITool } from "openai/resources/chat/completions"
import { buildExternalBasicHeaders } from "@/services/EnvUtils"
import { DiracStorageMessage } from "@/shared/messages/content"
import { fetch } from "@/shared/net"
import { ApiHandler, CommonApiHandlerOptions } from "../"
import { withRetry } from "../retry"
import { convertToOpenAiMessages } from "../transform/openai-format"
import { addReasoningContent } from "../transform/r1-format"
import { normalizeOpenaiReasoningEffort } from "@shared/storage/types"
import { ApiStream } from "../transform/stream"
import { getOpenAIToolParams, ToolCallProcessor } from "../transform/tool-call-processor"

interface DeepSeekHandlerOptions extends CommonApiHandlerOptions {
	deepSeekApiKey?: string
	reasoningEffort?: string
	apiModelId?: string
}

export class DeepSeekHandler implements ApiHandler {
	private options: DeepSeekHandlerOptions
	private client: OpenAI | undefined

	constructor(options: DeepSeekHandlerOptions) {
		this.options = options
	}

	private ensureClient(): OpenAI {
		if (!this.client) {
			if (!this.options.deepSeekApiKey) {
				throw new Error("DeepSeek API key is required")
			}
			try {
				this.client = new OpenAI({
					baseURL: "https://api.deepseek.com/v1",
					apiKey: this.options.deepSeekApiKey,
					defaultHeaders: buildExternalBasicHeaders(),
					fetch, // Use configured fetch with proxy support
				})
			} catch (error) {
				throw new Error(`Error creating DeepSeek client: ${error.message}`)
			}
		}
		return this.client
	}

	private async *yieldUsage(info: ModelInfo, usage: OpenAI.Completions.CompletionUsage | undefined): ApiStream {
		// Deepseek reports total input AND cache reads/writes,
		// see context caching: https://api-docs.deepseek.com/guides/kv_cache)
		// DeepSeek reports prompt_tokens as the sum of prompt_cache_hit_tokens and prompt_cache_miss_tokens.
		// We yield the total input tokens to ensure correct cost calculation and UI display.
		// where the input tokens is the sum of the cache hits/misses, just like OpenAI.
		// This affects:
		// 1) context management truncation algorithm, and
		// 2) cost calculation

		// Deepseek usage includes extra fields.
		// Safely cast the prompt token details section to the appropriate structure.
		interface DeepSeekUsage extends OpenAI.CompletionUsage {
			prompt_cache_hit_tokens?: number
			prompt_cache_miss_tokens?: number
		}
		const deepUsage = usage as DeepSeekUsage

		const inputTokens = deepUsage?.prompt_tokens || 0 // sum of cache hits and misses
		const outputTokens = deepUsage?.completion_tokens || 0
		const cacheReadTokens = deepUsage?.prompt_cache_hit_tokens || 0
		const cacheWriteTokens = deepUsage?.prompt_cache_miss_tokens || 0
		const totalCost = calculateApiCostOpenAI(info, inputTokens, outputTokens, cacheWriteTokens, cacheReadTokens)
		yield {
			type: "usage",
			inputTokens: inputTokens,
			outputTokens: outputTokens,
			cacheWriteTokens: cacheWriteTokens,
			cacheReadTokens: cacheReadTokens,
			totalCost: totalCost,
		}
	}

	@withRetry()
	async *createMessage(systemPrompt: string, messages: DiracStorageMessage[], tools?: OpenAITool[]): ApiStream {
		const client = this.ensureClient()
		const model = this.getModel()

		const isR1 = model.id.includes("reasoner") || model.id.includes("r1")
		const supportsReasoning = model.info.supportsReasoning
		const requestedEffort = normalizeOpenaiReasoningEffort(this.options.reasoningEffort)
		const isThinkingEnabled = supportsReasoning && requestedEffort !== "none"
		const useReasoningFormat = isR1 || isThinkingEnabled
		const shouldAddReasoningContent = isR1 || supportsReasoning
		const convertedMessages = convertToOpenAiMessages(messages)

		const openAiMessages: OpenAI.Chat.ChatCompletionMessageParam[] = shouldAddReasoningContent
			? [{ role: "system", content: systemPrompt }, ...addReasoningContent(convertedMessages, messages)]
			: [{ role: "system", content: systemPrompt }, ...convertedMessages]

		// DeepSeek.com API requires reasoning_content to be passed back for ALL assistant messages
		// and content to be at least an empty string (not null).
		const deepSeekMessages = openAiMessages.map((msg) => {
			if (msg.role === "assistant") {
				return {
					...msg,
					content: msg.content ?? "",
					reasoning_content: (msg as any).reasoning_content ?? "",
				}
			}
			return msg
		})

		const deepSeekTools = tools?.map((tool) => {
			if (tool.type === "function") {
				return {
					...tool,
					function: {
						...tool.function,
						strict: true,
					},
				}
			}
			return tool
		})

		const stream = await client.chat.completions.create({
			model: model.id,
			max_completion_tokens: model.info.maxTokens,
			messages: deepSeekMessages,
			stream: true,
			stream_options: { include_usage: true },
			...(supportsReasoning && !isR1
				? {
						// @ts-ignore
						extra_body: { thinking: { type: isThinkingEnabled ? "enabled" : "disabled" } },
						...(isThinkingEnabled ? { reasoning_effort: requestedEffort } : {}),
					}
				: {}),
			...(useReasoningFormat ? {} : { temperature: 0 }),
			...getOpenAIToolParams(deepSeekTools),
		})

		const toolCallProcessor = new ToolCallProcessor()

		for await (const chunk of stream) {
			const delta = chunk.choices?.[0]?.delta
			if (delta?.content) {
				yield {
					type: "text",
					text: delta.content,
				}
			}

			if (delta?.tool_calls) {
				yield* toolCallProcessor.processToolCallDeltas(delta.tool_calls)
			}

			if (delta && "reasoning_content" in delta && delta.reasoning_content) {
				yield {
					type: "reasoning",
					reasoning: (delta.reasoning_content as string | undefined) || "",
				}
			}

			if (chunk.usage) {
				yield* this.yieldUsage(model.info, chunk.usage)
			}
		}
	}

	getModel(): { id: DeepSeekModelId; info: ModelInfo } {
		const modelId = this.options.apiModelId
		if (modelId && modelId in deepSeekModels) {
			const id = modelId as DeepSeekModelId
			return { id, info: deepSeekModels[id] }
		}
		return {
			id: deepSeekDefaultModelId,
			info: deepSeekModels[deepSeekDefaultModelId],
		}
	}
}
