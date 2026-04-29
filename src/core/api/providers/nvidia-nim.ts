import { type ModelInfo, openAiModelInfoSaneDefaults } from "@shared/api"
import OpenAI from "openai"
import type { ChatCompletionTool as OpenAITool } from "openai/resources/chat/completions"
import { DiracStorageMessage } from "@/shared/messages/content"
import { createOpenAIClient } from "@/shared/net"
import { ApiHandler, CommonApiHandlerOptions } from "../index"
import { withRetry } from "../retry"
import { convertToOpenAiMessages } from "../transform/openai-format"
import { ApiStream } from "../transform/stream"
import { getOpenAIToolParams, ToolCallProcessor } from "../transform/tool-call-processor"

interface NvidiaNimHandlerOptions extends CommonApiHandlerOptions {
	nvidiaNimApiKey?: string
	nvidiaNimBaseUrl?: string
	apiModelId?: string
}

export class NvidiaNimHandler implements ApiHandler {
	private client: OpenAI | undefined

	constructor(private readonly options: NvidiaNimHandlerOptions) {}

	private ensureClient(): OpenAI {
		if (!this.client) {
			this.client = createOpenAIClient({
				baseURL: this.options.nvidiaNimBaseUrl || "https://integrate.api.nvidia.com/v1",
				apiKey: this.options.nvidiaNimApiKey || "not-used",
			})
		}
		return this.client
	}

	@withRetry()
	async *createMessage(systemPrompt: string, messages: DiracStorageMessage[], tools?: OpenAITool[]): ApiStream {
		const client = this.ensureClient()
		const model = this.getModel()

		const convertedMessages = convertToOpenAiMessages(messages)
		const openAiMessages: OpenAI.Chat.ChatCompletionMessageParam[] = [
			{ role: "system", content: systemPrompt },
			...convertedMessages,
		]

		const stream = await client.chat.completions.create({
			model: model.id,
			messages: openAiMessages,
			temperature: 0,
			stream: true,
			stream_options: { include_usage: true },
			...getOpenAIToolParams(tools),
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
				yield {
					type: "usage",
					inputTokens: chunk.usage.prompt_tokens || 0,
					outputTokens: chunk.usage.completion_tokens || 0,
				}
			}
		}
	}

	getModel(): { id: string; info: ModelInfo } {
		const modelId = this.options.apiModelId
		if (modelId) {
			return { id: modelId, info: openAiModelInfoSaneDefaults }
		}
		return { id: "nvidia/llama-3.1-nemotron-ultra-253b-v1", info: openAiModelInfoSaneDefaults }
	}
}
