import { ApiFormat } from "./proto/dirac/models"
import type { ApiHandlerSettings } from "./storage/state-keys"

export type ApiProvider =
	| "anthropic"
	| "claude-code"
	| "openrouter"
	| "openai"
	| "lmstudio"
	| "gemini"
	| "together"
	| "deepseek"
	| "qwen"
	| "mistral"
	| "vscode-lm"
	| "moonshot"
	| "nebius"
	| "fireworks"
	| "xai"
	| "sambanova"
	| "cerebras"
	| "groq"
	| "huggingface"
	| "zai"
	| "oca"
	| "minimax"
	| "nvidia-nim"
	| "opencode_go"
	| "opencode_zen"
	| "kilocode"
	| "byteplus"
	| "byteplus_coding_plan"
	| "openai_codex"
	| "xiaomi_mimo"
	| "synthetic"
	| "wafer"
	| "venice"
	| "inference_net"
	| "ovhcloud"
	| "ollama"
	| "replicate"

export const ALL_PROVIDERS: ApiProvider[] = [
	"anthropic",
	"claude-code",
	"openrouter",
	"openai",
	"lmstudio",
	"gemini",
	"together",
	"deepseek",
	"qwen",
	"mistral",
	"vscode-lm",
	"moonshot",
	"nebius",
	"fireworks",
	"xai",
	"sambanova",
	"cerebras",
	"groq",
	"huggingface",
	"zai",
	"oca",
	"minimax",
	"nvidia-nim",
	"opencode_go",
	"opencode_zen",
	"kilocode",
	"byteplus",
	"byteplus_coding_plan",
	"openai_codex",
	"xiaomi_mimo",
	"synthetic",
	"wafer",
	"venice",
	"inference_net",
	"ovhcloud",
	"ollama",
	"replicate",
]

export const DEFAULT_API_PROVIDER = "openrouter" as ApiProvider

export interface ApiHandlerOptions extends Partial<ApiHandlerSettings> {
	ulid?: string // Used to identify the task in API requests
	geminiSearchEnabled?: boolean

	onRetryAttempt?: (attempt: number, maxRetries: number, delay: number, error: any) => void // Callback function
}

export type ApiConfiguration = ApiHandlerOptions

// Models

export interface ModelInfo {
	name?: string
	maxTokens?: number
	contextWindow?: number
	supportsImages?: boolean
	supportsPromptCache: boolean // this value is hardcoded for now
	supportsReasoning?: boolean // Whether the model supports reasoning/thinking mode
	thinkingConfig?: {
		maxBudget?: number // Max allowed thinking budget tokens
		geminiThinkingLevel?: "low" | "high" // Optional: preset thinking level
		supportsThinkingLevel?: boolean // Whether the model supports thinking level (low/high)
	}
	supportsGlobalEndpoint?: boolean // Whether the model supports a global endpoint with Vertex AI
	description?: string
	temperature?: number
	supportsTools?: boolean

	apiFormat?: ApiFormat // The API format used by this model
}

export interface OpenAiCompatibleModelInfo extends ModelInfo {
	temperature?: number
	isR1FormatRequired?: boolean
	systemRole?: "developer" | "system"
	supportsReasoningEffort?: boolean
	supportsStreaming?: boolean
}

export interface OcaModelInfo extends OpenAiCompatibleModelInfo {
	modelName: string
	surveyId?: string
	banner?: string
	surveyContent?: string
	supportsReasoning?: boolean
	reasoningEffortOptions: string[]
}

export const CLAUDE_SONNET_1M_SUFFIX = ":1m"
export const ANTHROPIC_FAST_MODE_SUFFIX = ":fast"
// Anthropic
// https://docs.anthropic.com/en/docs/about-claude/models // prices updated 2025-01-02
export type AnthropicModelId = keyof typeof anthropicModels
export const anthropicDefaultModelId: AnthropicModelId = "claude-sonnet-4-6"
export const ANTHROPIC_MIN_THINKING_BUDGET = 1_024
export const ANTHROPIC_MAX_THINKING_BUDGET = 6_000
export const anthropicModels = {
	"claude-sonnet-4-6": {
		maxTokens: 64_000,
		contextWindow: 200_000,
		supportsImages: true,
		supportsPromptCache: true,
		supportsReasoning: true,
	},
	"claude-sonnet-4-6:1m": {
		maxTokens: 64_000,
		contextWindow: 1_000_000,
		supportsImages: true,
		supportsPromptCache: true,
		supportsReasoning: true,
	},
	"claude-haiku-4-5-20251001": {
		maxTokens: 64_000,
		contextWindow: 200_000,
		supportsImages: true,
		supportsPromptCache: true,
		supportsReasoning: true,
	},
	"claude-opus-4-6": {
		maxTokens: 128_000,
		contextWindow: 200_000,
		supportsImages: true,
		supportsPromptCache: true,
		supportsReasoning: true,
	},
	"claude-opus-4-6:fast": {
		maxTokens: 128_000,
		contextWindow: 200_000,
		supportsImages: true,
		supportsPromptCache: true,
		supportsReasoning: true,
		description:
			"Anthropic fast mode preview for Claude Opus 4.6. Same model and capabilities with higher output token speed at premium pricing. Requires fast mode access on your Anthropic account.",
	},
	"claude-opus-4-6:1m": {
		maxTokens: 128_000,
		contextWindow: 1_000_000,
		supportsImages: true,
		supportsPromptCache: true,
		supportsReasoning: true,
	},
	"claude-opus-4-6:1m:fast": {
		maxTokens: 128_000,
		contextWindow: 1_000_000,
		supportsImages: true,
		supportsPromptCache: true,
		supportsReasoning: true,
		description:
			"Anthropic fast mode preview for Claude Opus 4.6 with the 1M context beta enabled. Same model and capabilities with higher output token speed at premium pricing across the full 1M context window. Requires both fast mode and 1M context access on your Anthropic account.",
	},
} as const satisfies Record<string, ModelInfo> // as const assertion makes the object deeply readonly

// Claude Code
export type ClaudeCodeModelId = keyof typeof claudeCodeModels
export const claudeCodeDefaultModelId: ClaudeCodeModelId = "claude-sonnet-4-6"
export const claudeCodeModels = {
	opus: {
		...anthropicModels["claude-opus-4-6"],
		supportsImages: false,
		supportsPromptCache: false,
	},
	"opus[1m]": {
		...anthropicModels["claude-opus-4-6:1m"],
		supportsImages: false,
		supportsPromptCache: false,
	},
	"claude-haiku-4-5-20251001": {
		...anthropicModels["claude-haiku-4-5-20251001"],
		supportsImages: false,
		supportsPromptCache: false,
	},
	"claude-sonnet-4-6": {
		...anthropicModels["claude-sonnet-4-6"],
		supportsImages: false,
		supportsPromptCache: false,
	},
	"claude-sonnet-4-6[1m]": {
		...anthropicModels["claude-sonnet-4-6:1m"],
		supportsImages: false,
		supportsPromptCache: false,
	},
	"claude-opus-4-6": {
		...anthropicModels["claude-opus-4-6"],
		supportsImages: false,
		supportsPromptCache: false,
	},
	"claude-opus-4-6[1m]": {
		...anthropicModels["claude-opus-4-6:1m"],
		supportsImages: false,
		supportsPromptCache: false,
	},
} as const satisfies Record<string, ModelInfo>

// OpenRouter
// https://openrouter.ai/models?order=newest&supported_parameters=tools
export const openRouterDefaultModelId = "anthropic/claude-sonnet-4.5" // will always exist in openRouterModels
export const openRouterClaudeSonnet41mModelId = `anthropic/claude-sonnet-4${CLAUDE_SONNET_1M_SUFFIX}`
export const openRouterClaudeSonnet451mModelId = `anthropic/claude-sonnet-4.5${CLAUDE_SONNET_1M_SUFFIX}`
export const openRouterClaudeSonnet461mModelId = `anthropic/claude-sonnet-4.6${CLAUDE_SONNET_1M_SUFFIX}`
export const openRouterClaudeOpus461mModelId = `anthropic/claude-opus-4.6${CLAUDE_SONNET_1M_SUFFIX}`
export const openRouterDefaultModelInfo: ModelInfo = {
	maxTokens: 64_000,
	contextWindow: 200_000,
	supportsImages: true,
	supportsPromptCache: true,
	description:
		"Claude Sonnet 4.5 delivers superior intelligence across coding, agentic search, and AI agent capabilities. It's a powerful choice for agentic coding, and can complete tasks across the entire software development lifecycle, from initial planning to bug fixes, maintenance to large refactors. It offers strong performance in both planning and solving for complex coding tasks, making it an ideal choice to power end-to-end software development processes.\n\nRead more in the [blog post here](https://www.anthropic.com/claude/sonnet)",
}

export const OPENROUTER_PROVIDER_PREFERENCES: Record<string, { order: string[]; allow_fallbacks: boolean }> = {
	// Exacto Providers
	"moonshotai/kimi-k2:exacto": {
		order: ["groq", "moonshotai"],
		allow_fallbacks: false,
	},
	"z-ai/glm-4.6:exacto": {
		order: ["z-ai", "novita"],
		allow_fallbacks: false,
	},
	"deepseek/deepseek-v3.1-terminus:exacto": {
		order: ["novita", "deepinfra"],
		allow_fallbacks: false,
	},
	"qwen/qwen3-coder:exacto": {
		order: ["baseten"],
		allow_fallbacks: false,
	},
	"openai/gpt-oss-120b:exacto": {
		order: ["groq", "novita"],
		allow_fallbacks: false,
	},

	// Normal Providers
	"moonshotai/kimi-k2": {
		order: ["groq", "fireworks", "baseten", "parasail", "novita", "deepinfra"],
		allow_fallbacks: false,
	},
	"qwen/qwen3-coder": {
		order: ["nebius", "baseten", "fireworks", "together", "deepinfra"],
		allow_fallbacks: false,
	},
	"qwen/qwen3-235b-a22b-thinking-2507": {
		order: ["nebius", "baseten", "fireworks", "together", "deepinfra"],
		allow_fallbacks: false,
	},
	"qwen/qwen3-235b-a22b-07-25": {
		order: ["nebius", "baseten", "fireworks", "together", "deepinfra"],
		allow_fallbacks: false,
	},
	"qwen/qwen3-30b-a3b-thinking-2507": {
		order: ["nebius", "baseten", "fireworks", "together", "deepinfra"],
		allow_fallbacks: false,
	},
	"qwen/qwen3-30b-a3b-instruct-2507": {
		order: ["nebius", "baseten", "fireworks", "together", "deepinfra"],
		allow_fallbacks: false,
	},
	"qwen/qwen3-30b-a3b:free": {
		order: ["nebius", "baseten", "fireworks", "together", "deepinfra"],
		allow_fallbacks: false,
	},
	"qwen/qwen3-next-80b-a3b-thinking": {
		order: ["nebius", "baseten", "fireworks", "together", "deepinfra"],
		allow_fallbacks: false,
	},
	"qwen/qwen3-next-80b-a3b-instruct": {
		order: ["nebius", "baseten", "fireworks", "together", "deepinfra"],
		allow_fallbacks: false,
	},
	"qwen/qwen3-max": {
		order: ["nebius", "baseten", "fireworks", "together", "deepinfra"],
		allow_fallbacks: false,
	},
	"deepseek/deepseek-v3.2-exp": {
		order: ["deepseek", "novita", "fireworks", "nebius"],
		allow_fallbacks: false,
	},
	"z-ai/glm-4.6": {
		order: ["z-ai", "novita", "baseten", "fireworks", "chutes"],
		allow_fallbacks: false,
	},
	"z-ai/glm-4.5v": {
		order: ["z-ai", "novita", "baseten", "fireworks", "chutes"],
		allow_fallbacks: false,
	},
	"z-ai/glm-4.5": {
		order: ["z-ai", "novita", "baseten", "fireworks", "chutes"],
		allow_fallbacks: false,
	},
	"z-ai/glm-4.5-air": {
		order: ["z-ai", "novita", "baseten", "fireworks", "chutes"],
		allow_fallbacks: false,
	},
}

export const openAiModelInfoSaneDefaults: OpenAiCompatibleModelInfo = {
	maxTokens: -1,
	contextWindow: 128_000,
	supportsImages: true,
	supportsPromptCache: false,
	supportsTools: true,
	supportsReasoning: true,
	isR1FormatRequired: false,
	temperature: 0,
}

// Azure OpenAI
// https://learn.microsoft.com/en-us/azure/ai-services/openai/api-version-deprecation
// https://learn.microsoft.com/en-us/azure/ai-services/openai/reference#api-specs
export const azureOpenAiDefaultApiVersion = "2024-08-01-preview"

// DeepSeek
// https://api-docs.deepseek.com/quick_start/pricing
export type DeepSeekModelId = keyof typeof deepSeekModels
export const deepSeekDefaultModelId: DeepSeekModelId = "deepseek-v4-pro"
export const deepSeekModels = {
	"deepseek-v4-flash": {
		maxTokens: 384_000,
		contextWindow: 1_048_576,
		supportsImages: false,
		supportsPromptCache: true, 
		supportsReasoning: true,
		supportsReasoningEffort: true,
		supportsTools: true,
	},
	"deepseek-v4-pro": {
		maxTokens: 384_000,
		contextWindow: 1_048_576,
		supportsImages: false,
		supportsPromptCache: true, 
		supportsReasoning: true,
		supportsReasoningEffort: true,
		supportsTools: true,
	},
	"deepseek-chat": {
		maxTokens: 8_000,
		contextWindow: 128_000,
		supportsImages: false,
		supportsPromptCache: true, // supports context caching, but not in the way anthropic does it (deepseek reports input tokens and reads/writes in the same usage report) FIXME: we need to show users cache stats how deepseek does it
	},
	"deepseek-reasoner": {
		maxTokens: 8_000,
		contextWindow: 128_000,
		supportsImages: false,
		supportsPromptCache: true,
		supportsReasoning: true,
		supportsTools: true,
	},
} as const satisfies Record<string, OpenAiCompatibleModelInfo>

// Hugging Face Inference Providers
// https://huggingface.co/docs/inference-providers/en/index
export type HuggingFaceModelId = keyof typeof huggingFaceModels
export const huggingFaceDefaultModelId: HuggingFaceModelId = "moonshotai/Kimi-K2-Instruct"
export const huggingFaceModels = {
	"openai/gpt-oss-120b": {
		maxTokens: 32766,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		description:
			"Large open-weight reasoning model for high-end desktops and data centers, built for complex coding, math, and general AI tasks.",
	},
	"openai/gpt-oss-20b": {
		maxTokens: 32766,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		description:
			"Medium open-weight reasoning model that runs on most desktops, balancing strong reasoning with broad accessibility.",
	},
	"moonshotai/Kimi-K2-Instruct": {
		supportsTools: true,
		maxTokens: 131_072,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		description: "Advanced reasoning model with superior performance across coding, math, and general capabilities.",
	},
	"deepseek-ai/DeepSeek-R1": {
		maxTokens: 8192,
		contextWindow: 64_000,
		supportsImages: false,
		supportsPromptCache: false,
		description: "DeepSeek's reasoning model with step-by-step thinking capabilities.",
	},
} as const satisfies Record<string, ModelInfo>

// Qwen
// https://bailian.console.aliyun.com/
// The first model in the list is used as the default model for each region
export const internationalQwenModels = {
	"qwen3-coder-plus": {
		maxTokens: 65_536,
		contextWindow: 1_000_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"qwen3-coder-480b-a35b-instruct": {
		maxTokens: 65_536,
		contextWindow: 204_800,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"qwen3-235b-a22b": {
		maxTokens: 16_384,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 38_912,
		},
	},
	"qwen3-32b": {
		maxTokens: 16_384,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 38_912,
		},
	},
	"qwen3-30b-a3b": {
		maxTokens: 16_384,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 38_912,
		},
	},
	"qwen3-14b": {
		maxTokens: 8_192,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 38_912,
		},
	},
	"qwen3-8b": {
		maxTokens: 8_192,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 38_912,
		},
	},
	"qwen3-4b": {
		maxTokens: 8_192,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 38_912,
		},
	},
	"qwen3-1.7b": {
		maxTokens: 8_192,
		contextWindow: 32_768,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 30_720,
		},
	},
	"qwen3-0.6b": {
		maxTokens: 8_192,
		contextWindow: 32_768,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 30_720,
		},
	},
	"qwen-coder-plus-latest": {
		maxTokens: 129_024,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"qwen-plus-latest": {
		maxTokens: 16_384,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 38_912,
		},
	},
	"qwen-turbo-latest": {
		maxTokens: 16_384,
		contextWindow: 1_000_000,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 38_912,
		},
	},
	"qwen-max-latest": {
		maxTokens: 30_720,
		contextWindow: 32_768,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"qwen-coder-plus": {
		maxTokens: 129_024,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"qwen-plus": {
		maxTokens: 129_024,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"qwen-turbo": {
		maxTokens: 1_000_000,
		contextWindow: 1_000_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"qwen-max": {
		maxTokens: 30_720,
		contextWindow: 32_768,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"deepseek-v3": {
		maxTokens: 8_000,
		contextWindow: 64_000,
		supportsImages: false,
		supportsPromptCache: true,
	},
	"deepseek-r1": {
		maxTokens: 8_000,
		contextWindow: 64_000,
		supportsImages: false,
		supportsPromptCache: true,
	},
	"qwen-vl-max": {
		maxTokens: 30_720,
		contextWindow: 32_768,
		supportsImages: true,
		supportsPromptCache: false,
	},
	"qwen-vl-max-latest": {
		maxTokens: 129_024,
		contextWindow: 131_072,
		supportsImages: true,
		supportsPromptCache: false,
	},
	"qwen-vl-plus": {
		maxTokens: 6_000,
		contextWindow: 8_000,
		supportsImages: true,
		supportsPromptCache: false,
	},
	"qwen-vl-plus-latest": {
		maxTokens: 129_024,
		contextWindow: 131_072,
		supportsImages: true,
		supportsPromptCache: false,
	},
} as const satisfies Record<string, ModelInfo>

export const mainlandQwenModels = {
	"qwen3-235b-a22b": {
		maxTokens: 16_384,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 38_912,
		},
	},
	"qwen3-32b": {
		maxTokens: 16_384,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 38_912,
		},
	},
	"qwen3-30b-a3b": {
		maxTokens: 16_384,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 38_912,
		},
	},
	"qwen3-14b": {
		maxTokens: 8_192,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 38_912,
		},
	},
	"qwen3-8b": {
		maxTokens: 8_192,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 38_912,
		},
	},
	"qwen3-4b": {
		maxTokens: 8_192,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 38_912,
		},
	},
	"qwen3-1.7b": {
		maxTokens: 8_192,
		contextWindow: 32_768,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 30_720,
		},
	},
	"qwen3-0.6b": {
		maxTokens: 8_192,
		contextWindow: 32_768,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 30_720,
		},
	},
	"qwen-coder-plus-latest": {
		maxTokens: 129_024,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"qwen-plus-latest": {
		maxTokens: 16_384,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 38_912,
		},
	},
	"qwen-turbo-latest": {
		maxTokens: 16_384,
		contextWindow: 1_000_000,
		supportsImages: false,
		supportsPromptCache: false,
		thinkingConfig: {
			maxBudget: 38_912,
		},
	},
	"qwen-max-latest": {
		maxTokens: 30_720,
		contextWindow: 32_768,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"qwq-plus-latest": {
		maxTokens: 8_192,
		contextWindow: 131_071,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"qwq-plus": {
		maxTokens: 8_192,
		contextWindow: 131_071,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"qwen-coder-plus": {
		maxTokens: 129_024,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"qwen-plus": {
		maxTokens: 129_024,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"qwen-turbo": {
		maxTokens: 1_000_000,
		contextWindow: 1_000_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"qwen-max": {
		maxTokens: 30_720,
		contextWindow: 32_768,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"deepseek-v3": {
		maxTokens: 8_000,
		contextWindow: 64_000,
		supportsImages: false,
		supportsPromptCache: true,
	},
	"deepseek-r1": {
		maxTokens: 8_000,
		contextWindow: 64_000,
		supportsImages: false,
		supportsPromptCache: true,
	},
	"qwen-vl-max": {
		maxTokens: 30_720,
		contextWindow: 32_768,
		supportsImages: true,
		supportsPromptCache: false,
	},
	"qwen-vl-max-latest": {
		maxTokens: 129_024,
		contextWindow: 131_072,
		supportsImages: true,
		supportsPromptCache: false,
	},
	"qwen-vl-plus": {
		maxTokens: 6_000,
		contextWindow: 8_000,
		supportsImages: true,
		supportsPromptCache: false,
	},
	"qwen-vl-plus-latest": {
		maxTokens: 129_024,
		contextWindow: 131_072,
		supportsImages: true,
		supportsPromptCache: false,
	},
} as const satisfies Record<string, ModelInfo>
export enum QwenApiRegions {
	CHINA = "china",
	INTERNATIONAL = "international",
}
export type MainlandQwenModelId = keyof typeof mainlandQwenModels
export type InternationalQwenModelId = keyof typeof internationalQwenModels
// Set first model in the list as the default model for each region
export const internationalQwenDefaultModelId: InternationalQwenModelId = Object.keys(
	internationalQwenModels,
)[0] as InternationalQwenModelId
export const mainlandQwenDefaultModelId: MainlandQwenModelId = Object.keys(mainlandQwenModels)[0] as MainlandQwenModelId

// Mistral
// https://docs.mistral.ai/getting-started/models/models_overview/
export type MistralModelId = keyof typeof mistralModels
export const mistralDefaultModelId: MistralModelId = "mistral-medium-latest"
export const mistralModels = {
	"devstral-2512": {
		maxTokens: 256_000,
		contextWindow: 256_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"labs-devstral-small-2512": {
		maxTokens: 256_000,
		contextWindow: 256_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"mistral-large-2512": {
		maxTokens: 256_000,
		contextWindow: 256_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"ministral-14b-2512": {
		maxTokens: 256_000,
		contextWindow: 256_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"mistral-small-latest": {
		maxTokens: 128_000,
		contextWindow: 128_000,
		supportsImages: true,
		supportsPromptCache: false,
	},
	"mistral-medium-latest": {
		maxTokens: 128_000,
		contextWindow: 128_000,
		supportsImages: true,
		supportsPromptCache: false,
	},
	"mistral-small-2501": {
		maxTokens: 32_000,
		contextWindow: 32_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"open-codestral-mamba": {
		maxTokens: 256_000,
		contextWindow: 256_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"codestral-2501": {
		maxTokens: 256_000,
		contextWindow: 256_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"devstral-small-2505": {
		maxTokens: 128_000,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"devstral-medium-latest": {
		maxTokens: 128_000,
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
	},
} as const satisfies Record<string, ModelInfo>

// Nebius AI Studio
// https://docs.nebius.com/studio/inference/models
export const nebiusModels = {
	"deepseek-ai/DeepSeek-V3": {
		maxTokens: 32_000,
		contextWindow: 96_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"deepseek-ai/DeepSeek-R1": {
		maxTokens: 32_000,
		contextWindow: 96_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"deepseek-ai/DeepSeek-R1-fast": {
		maxTokens: 32_000,
		contextWindow: 96_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"meta-llama/Llama-3.3-70B-Instruct-fast": {
		maxTokens: 32_000,
		contextWindow: 96_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"Qwen/Qwen2.5-32B-Instruct-fast": {
		maxTokens: 8_192,
		contextWindow: 32_768,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"Qwen/Qwen2.5-Coder-32B-Instruct-fast": {
		maxTokens: 128_000,
		contextWindow: 128_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"Qwen/Qwen3-4B-fast": {
		maxTokens: 32_000,
		contextWindow: 41_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"Qwen/Qwen3-30B-A3B-fast": {
		maxTokens: 32_000,
		contextWindow: 41_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"Qwen/Qwen3-235B-A22B": {
		maxTokens: 32_000,
		contextWindow: 41_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"openai/gpt-oss-120b": {
		maxTokens: 32766, // Quantization: fp4
		contextWindow: 131_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"moonshotai/Kimi-K2-Instruct": {
		supportsTools: true,
		maxTokens: 16384, // Quantization: fp4
		contextWindow: 131_000,
		supportsImages: false,
		supportsPromptCache: true,
	},
	"Qwen/Qwen3-Coder-480B-A35B-Instruct": {
		maxTokens: 163800, // Quantization: fp8
		contextWindow: 262_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"openai/gpt-oss-20b": {
		maxTokens: 32766, // Quantization: fp4
		contextWindow: 131_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"zai-org/GLM-4.5": {
		supportsTools: true,
		maxTokens: 98304, // Quantization: fp8
		contextWindow: 128_000,
		supportsImages: false,
		supportsPromptCache: true,
	},
	"zai-org/GLM-4.5-Air": {
		supportsTools: true,
		maxTokens: 98304, // Quantization: fp8
		contextWindow: 128_000,
		supportsImages: false,
		supportsPromptCache: true,
	},
	"deepseek-ai/DeepSeek-R1-0528-fast": {
		maxTokens: 128000, // Quantization: fp4
		contextWindow: 164_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"Qwen/Qwen3-235B-A22B-Instruct-2507": {
		maxTokens: 64000, // Quantization: fp8
		contextWindow: 262_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"Qwen/Qwen3-30B-A3B": {
		maxTokens: 32000, // Quantization: fp8
		contextWindow: 41_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"Qwen/Qwen3-32B": {
		maxTokens: 16384, // Quantization: fp8
		contextWindow: 41_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
	"Qwen/Qwen3-32B-fast": {
		maxTokens: 16384, // Quantization: fp8
		contextWindow: 41_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
} as const satisfies Record<string, ModelInfo>
export type NebiusModelId = keyof typeof nebiusModels
export const nebiusDefaultModelId = "Qwen/Qwen2.5-32B-Instruct-fast" satisfies NebiusModelId

// X AI
// https://docs.x.ai/docs/api-reference
export type XAIModelId = keyof typeof xaiModels
export const xaiDefaultModelId: XAIModelId = "grok-4"
export const xaiModels = {
	"grok-4-1-fast-reasoning": {
		contextWindow: 2_000_000,
		supportsImages: false,
		supportsPromptCache: true,
		description: "xAI's Grok 4.1 Reasoning Fast - multimodal model with 2M context.",
	},
	"grok-4-1-fast-non-reasoning": {
		contextWindow: 2_000_000,
		supportsImages: true,
		supportsPromptCache: true,
		description: "xAI's Grok 4.1 Non-Reasoning Fast - multimodal model with 2M context.",
	},
	"grok-code-fast-1": {
		contextWindow: 256_000,
		supportsImages: false,
		supportsPromptCache: true,
		description: "xAI's Grok Coding model.",
	},
	"grok-4-fast-reasoning": {
		maxTokens: 30000,
		contextWindow: 2000000,
		supportsImages: true,
		supportsPromptCache: false,
		description: "xAI's Grok 4 Fast (free) multimodal model with 2M context.",
	},
	"grok-4": {
		maxTokens: 8192,
		contextWindow: 262144,
		supportsImages: true,
		supportsPromptCache: true,
	},
	"grok-3-fast": {
		maxTokens: 8192,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: true,
		description: "X AI's Grok-3 fast model with 131K context window",
	},
	"grok-3-mini": {
		maxTokens: 8192,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: true,
		description: "X AI's Grok-3 mini model with 131K context window",
	},
	"grok-3-mini-fast": {
		maxTokens: 8192,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: true,
		description: "X AI's Grok-3 mini fast model with 131K context window",
	},
	"grok-2-latest": {
		maxTokens: 8192,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: false,
		description: "X AI's Grok-2 model - latest version with 131K context window",
	},
	"grok-2-1212": {
		maxTokens: 8192,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: false,
		description: "X AI's Grok-2 model (version 1212) with 131K context window",
	},
	"grok-2-vision-latest": {
		maxTokens: 8192,
		contextWindow: 32768,
		supportsImages: true,
		supportsPromptCache: false,
		description: "X AI's Grok-2 Vision model - latest version with image support and 32K context window",
	},
	"grok-2-vision": {
		maxTokens: 8192,
		contextWindow: 32768,
		supportsImages: true,
		supportsPromptCache: false,
		description: "X AI's Grok-2 Vision model with image support and 32K context window",
	},
	"grok-2-vision-1212": {
		maxTokens: 8192,
		contextWindow: 32768,
		supportsImages: true,
		supportsPromptCache: false,
		description: "X AI's Grok-2 Vision model (version 1212) with image support and 32K context window",
	},
} as const satisfies Record<string, ModelInfo>

// SambaNova
// https://docs.sambanova.ai/cloud/docs/get-started/supported-models
export type SambanovaModelId = keyof typeof sambanovaModels
export const sambanovaDefaultModelId: SambanovaModelId = "Meta-Llama-3.3-70B-Instruct"
export const sambanovaModels = {
	"DeepSeek-R1-0528": {
		maxTokens: 7168,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: false,
		temperature: 0.6,
	},
	"DeepSeek-R1-Distill-Llama-70B": {
		maxTokens: 4096,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: false,
		temperature: 0.6,
	},
	"DeepSeek-V3-0324": {
		maxTokens: 7168,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: false,
		temperature: 0.3,
	},
	"DeepSeek-V3.1": {
		maxTokens: 7168,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: false,
		temperature: 0.6,
	},
	"DeepSeek-V3.1-Terminus": {
		maxTokens: 7168,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: false,
		temperature: 0.6,
	},
	"Llama-4-Maverick-17B-128E-Instruct": {
		maxTokens: 4096,
		contextWindow: 131072,
		supportsImages: true,
		supportsPromptCache: false,
		temperature: 0.6,
	},
	"Meta-Llama-3.1-8B-Instruct": {
		maxTokens: 4096,
		contextWindow: 16384,
		supportsImages: false,
		supportsPromptCache: false,
		temperature: 0.6,
	},
	"Meta-Llama-3.3-70B-Instruct": {
		maxTokens: 3072,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: false,
		temperature: 0.6,
	},
	"MiniMax-M2.5": {
		supportsTools: true,
		maxTokens: 16384,
		contextWindow: 163840,
		supportsImages: false,
		supportsPromptCache: false,
		temperature: 1.0,
	},
	"Qwen3-235B": {
		maxTokens: 4096,
		contextWindow: 65536,
		supportsImages: false,
		supportsPromptCache: false,
		temperature: 0.7,
	},
	"Qwen3-32B": {
		maxTokens: 4096,
		contextWindow: 32768,
		supportsImages: false,
		supportsPromptCache: false,
		temperature: 0.6,
	},
} as const satisfies Record<string, ModelInfo>

// Cerebras
// https://inference-docs.cerebras.ai/api-reference/models
export type CerebrasModelId = keyof typeof cerebrasModels
export const cerebrasDefaultModelId: CerebrasModelId = "zai-glm-4.7"
export const cerebrasModels = {
	"zai-glm-4.7": {
		supportsTools: true,
		maxTokens: 40000,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: false,
		temperature: 0.9,
		description:
			"Highly capable general-purpose model on Cerebras (up to 1,000 tokens/s), competitive with leading proprietary models on coding tasks.",
	},
	"gpt-oss-120b": {
		maxTokens: 65536,
		contextWindow: 128000,
		supportsImages: false,
		supportsPromptCache: false,
		description: "Intelligent general purpose model with 3,000 tokens/s",
	},
	"qwen-3-235b-a22b-instruct-2507": {
		maxTokens: 64000,
		contextWindow: 64000,
		supportsImages: false,
		supportsPromptCache: false,
		description: "Intelligent model with ~1400 tokens/s",
	},
} as const satisfies Record<string, ModelInfo>

// Groq
// https://console.groq.com/docs/models
// https://groq.com/pricing/
export type GroqModelId = keyof typeof groqModels
export const groqDefaultModelId: GroqModelId = "qwen/qwen3-32b"
export const groqModels = {
	"openai/gpt-oss-120b": {
		maxTokens: 32766, // Model fails if you try to use more than 32K tokens
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		description:
			"A state-of-the-art 120B open-weight Mixture-of-Experts language model optimized for strong reasoning, tool use, and efficient deployment on large GPUs",
	},
	"openai/gpt-oss-20b": {
		maxTokens: 32766, // Model fails if you try to use more than 32K tokens
		contextWindow: 131_072,
		supportsImages: false,
		supportsPromptCache: false,
		description:
			"A compact 20B open-weight Mixture-of-Experts language model designed for strong reasoning and tool use, ideal for edge devices and local inference.",
	},
	// Compound Beta Models - Hybrid architectures optimized for tool use
	"compound-beta": {
		maxTokens: 8192,
		contextWindow: 128000,
		supportsImages: false,
		supportsPromptCache: false,
		description:
			"Compound model using Llama 4 Scout for core reasoning with Llama 3.3 70B for routing and tool use. Excellent for plan/act workflows.",
	},
	"compound-beta-mini": {
		maxTokens: 8192,
		contextWindow: 128000,
		supportsImages: false,
		supportsPromptCache: false,
		description: "Lightweight compound model for faster inference while maintaining tool use capabilities.",
	},
	// DeepSeek Models - Reasoning-optimized
	"deepseek-r1-distill-llama-70b": {
		maxTokens: 131072,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: false,
		description:
			"DeepSeek R1 reasoning capabilities distilled into Llama 70B architecture. Excellent for complex problem-solving and planning.",
	},
	// Llama 4 Models
	"meta-llama/llama-4-maverick-17b-128e-instruct": {
		maxTokens: 8192,
		contextWindow: 131072,
		supportsImages: true,
		supportsPromptCache: false,
		description: "Meta's Llama 4 Maverick 17B model with 128 experts, supports vision and multimodal tasks.",
	},
	"meta-llama/llama-4-scout-17b-16e-instruct": {
		maxTokens: 8192,
		contextWindow: 131072,
		supportsImages: true,
		supportsPromptCache: false,
		description: "Meta's Llama 4 Scout 17B model with 16 experts, optimized for fast inference and general tasks.",
	},
	// Llama 3.3 Models
	"llama-3.3-70b-versatile": {
		maxTokens: 32768,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: false,
		description: "Meta's latest Llama 3.3 70B model optimized for versatile use cases with excellent performance and speed.",
	},
	// Llama 3.1 Models - Fast inference
	"llama-3.1-8b-instant": {
		maxTokens: 131072,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: false,
		description: "Fast and efficient Llama 3.1 8B model optimized for speed, low latency, and reliable tool execution.",
	},
	// Moonshot Models
	"moonshotai/kimi-k2-instruct": {
		isR1FormatRequired: true,
		supportsTools: true,
		maxTokens: 16384,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: true,
		description:
			"Kimi K2 is Moonshot AI's state-of-the-art Mixture-of-Experts (MoE) language model with 1 trillion total parameters and 32 billion activated parameters.",
	},
	"qwen/qwen3-32b": {
		isR1FormatRequired: true,
		supportsTools: true,
		maxTokens: 16384,
		contextWindow: 262144,
		supportsImages: false,
		supportsPromptCache: true,
		description:
			"Qwen3 32B dense model with strong general-purpose reasoning and tool use. 262K context window.",
	},
} as const satisfies Record<string, OpenAiCompatibleModelInfo>

// Moonshot AI Studio
// https://platform.moonshot.ai/docs/pricing/chat
export const moonshotModels = {
	"kimi-k2.6": {
		maxTokens: 32_000,
		contextWindow: 262_144,
		supportsImages: true,
		supportsReasoning: true,
		supportsPromptCache: true,
		temperature: 1.0,
		isR1FormatRequired: true,
		supportsTools: true,
	},
	"kimi-k2.5": {
		maxTokens: 32_000,
		contextWindow: 262_144,
		supportsImages: true,
		supportsReasoning: true,
		supportsPromptCache: true,
		temperature: 1.0,
		isR1FormatRequired: true,
		supportsTools: true,
	},
	"kimi-k2-0905-preview": {
		maxTokens: 16384,
		contextWindow: 262144,
		supportsImages: false,
		supportsReasoning: true,
		supportsPromptCache: false,
		temperature: 0.6,
		isR1FormatRequired: true,
		supportsTools: true,
	},
	"kimi-k2-thinking-turbo": {
		maxTokens: 32_000,
		contextWindow: 262_144,
		supportsImages: false,
		supportsReasoning: true,
		supportsPromptCache: false,
		temperature: 1.0,
		isR1FormatRequired: true,
		supportsTools: true,
	},
} as const satisfies Record<string, OpenAiCompatibleModelInfo>
export type MoonshotModelId = keyof typeof moonshotModels
export const moonshotDefaultModelId = "kimi-k2.6" satisfies MoonshotModelId

// Z AI
// https://docs.z.ai/guides/llm/glm-5
// https://docs.z.ai/guides/overview/pricing
export type internationalZAiModelId = keyof typeof internationalZAiModels
export const internationalZAiDefaultModelId: internationalZAiModelId = "glm-5"
export const internationalZAiModels = {
	"glm-5.1": {
		supportsTools: true,
		maxTokens: 128_000,
		contextWindow: 200_000,
		supportsImages: false,
		supportsPromptCache: true,
		supportsReasoning: true,
		thinkingConfig: {
			maxBudget: 128_000,
		},
	},
	"glm-5": {
		supportsTools: true,
		maxTokens: 128_000,
		contextWindow: 200_000,
		supportsImages: false,
		supportsPromptCache: true,
		supportsReasoning: true,
		thinkingConfig: {
			maxBudget: 128_000,
		},
	},
	"glm-4.7": {
		supportsTools: true,
		maxTokens: 128_000,
		contextWindow: 200_000,
		supportsImages: false,
		supportsPromptCache: true,
		supportsReasoning: true,
		thinkingConfig: {
			maxBudget: 128_000,
		},
	},
	"glm-4.6": {
		supportsTools: true,
		maxTokens: 128_000,
		contextWindow: 200_000,
		supportsImages: false,
		supportsPromptCache: true,
		supportsReasoning: true,
		thinkingConfig: {
			maxBudget: 128_000,
		},
	},
} as const satisfies Record<string, ModelInfo>

export type mainlandZAiModelId = keyof typeof mainlandZAiModels
export const mainlandZAiDefaultModelId: mainlandZAiModelId = "glm-5"
export const mainlandZAiModels = {
	"glm-5.1": {
		supportsTools: true,
		maxTokens: 128_000,
		contextWindow: 200_000,
		supportsImages: false,
		supportsPromptCache: true,
		supportsReasoning: true,
		thinkingConfig: {
			maxBudget: 128_000,
		},
	},
	"glm-5": {
		supportsTools: true,
		maxTokens: 128_000,
		contextWindow: 200_000,
		supportsImages: false,
		supportsPromptCache: true,
		supportsReasoning: true,
		thinkingConfig: {
			maxBudget: 128_000,
		},
	},
	"glm-4.7": {
		supportsTools: true,
		maxTokens: 128_000,
		contextWindow: 200_000,
		supportsImages: false,
		supportsPromptCache: true,
		supportsReasoning: true,
		thinkingConfig: {
			maxBudget: 128_000,
		},
	},
	"glm-4.6": {
		supportsTools: true,
		maxTokens: 128_000,
		contextWindow: 200_000,
		supportsImages: false,
		supportsPromptCache: true,
		supportsReasoning: true,
		thinkingConfig: {
			maxBudget: 128_000,
		},
	},
} as const satisfies Record<string, OpenAiCompatibleModelInfo>

// Z AI Coding Plan (subscription, free-form model ID)
export const codingPlanZAiModelInfoSaneDefaults: ModelInfo = {
	maxTokens: 128_000,
	contextWindow: 200_000,
	supportsImages: false,
	supportsPromptCache: true,
	supportsReasoning: true,
	thinkingConfig: { maxBudget: 128_000 },
}

// Fireworks AI
export type FireworksModelId = keyof typeof fireworksModels
export const fireworksDefaultModelId: FireworksModelId = "accounts/fireworks/models/kimi-k2p6"
export const fireworksModels = {
	"accounts/fireworks/models/kimi-k2p6": {
		isR1FormatRequired: true,
		supportsTools: true,
		maxTokens: 16384,
		contextWindow: 262144,
		supportsImages: true,
		supportsReasoning: true,
		supportsPromptCache: true,
		description:
			"Moonshot's flagship open agentic model. Kimi K2.5 unifies vision and text, thinking and non-thinking modes, and single-agent and multi-agent execution.",
	},
	"accounts/fireworks/models/kimi-k2p5": {
		isR1FormatRequired: true,
		supportsTools: true,
		maxTokens: 16384,
		contextWindow: 262144,
		supportsImages: true,
		supportsReasoning: true,
		supportsPromptCache: true,
		description:
			"Moonshot's flagship open agentic model. Kimi K2.5 unifies vision and text, thinking and non-thinking modes, and single-agent and multi-agent execution.",
	},
	"accounts/fireworks/models/qwen3-vl-30b-a3b-thinking": {
		maxTokens: 32768,
		contextWindow: 262144,
		supportsImages: true,
		supportsPromptCache: true,
		description:
			"Reasoning-enabled Qwen3-VL model with strong multimodal understanding, long context support, and function calling.",
	},
	"accounts/fireworks/models/qwen3-vl-30b-a3b-instruct": {
		maxTokens: 32768,
		contextWindow: 262144,
		supportsImages: true,
		supportsPromptCache: false,
		description: "Qwen3-VL instruct model with strong multimodal reasoning, long context support, and function calling.",
	},
	"accounts/fireworks/models/deepseek-v3p2": {
		maxTokens: 16384,
		contextWindow: 163840,
		supportsImages: false,
		supportsPromptCache: true,
		description: "DeepSeek V3.2 model tuned for high computational efficiency and strong reasoning and agent performance.",
	},
	"accounts/fireworks/models/glm-4p7": {
		supportsTools: true,
		maxTokens: 16384,
		contextWindow: 202752,
		supportsImages: false,
		supportsReasoning: true,
		supportsPromptCache: true,
		description: "GLM-4.7 is a next-generation general-purpose model optimized for coding, reasoning, and agentic workflows.",
	},
	"accounts/fireworks/models/glm-5": {
		supportsTools: true,
		maxTokens: 16384,
		contextWindow: 202752,
		supportsImages: false,
		supportsReasoning: true,
		supportsPromptCache: true,
		description: "GLM-5 is Z.ai's flagship reasoning model for complex systems engineering and long-horizon agentic tasks.",
	},
	"accounts/fireworks/models/minimax-m2p5": {
		supportsTools: true,
		maxTokens: 16384,
		contextWindow: 196608,
		supportsImages: false,
		supportsReasoning: true,
		supportsPromptCache: true,
		description: "MiniMax M2.5 is built for state-of-the-art coding, agentic tool use.",
	},
	"accounts/fireworks/models/minimax-m2p1": {
		supportsTools: true,
		maxTokens: 16384,
		contextWindow: 196608,
		supportsImages: false,
		supportsReasoning: true,
		supportsPromptCache: true,
		description:
			"MiniMax M2.1 is tuned for strong real-world performance across coding, agent-driven, and workflow-heavy tasks.",
	},
	"accounts/fireworks/models/gpt-oss-120b": {
		maxTokens: 16384,
		contextWindow: 131072,
		supportsImages: false,
		supportsPromptCache: true,
		description: "OpenAI gpt-oss-120b open-weight model for production and high-reasoning use cases.",
	},
} as const satisfies Record<string, OpenAiCompatibleModelInfo>

// Minimax
// https://www.minimax.io/platform/document/text_api_intro
// https://www.minimax.io/platform/document/pricing
export type MinimaxModelId = keyof typeof minimaxModels
export const minimaxDefaultModelId: MinimaxModelId = "MiniMax-M2.7"
export const minimaxModels = {
	"MiniMax-M2.7": {
		supportsTools: true,
		maxTokens: 128_000,
		contextWindow: 192_000,
		supportsImages: false,
		supportsPromptCache: true,
		supportsReasoning: true,
		description: "Latest flagship model with enhanced reasoning and coding",
	},
	"MiniMax-M2.7-highspeed": {
		supportsTools: true,
		maxTokens: 128_000,
		contextWindow: 192_000,
		supportsImages: false,
		supportsPromptCache: true,
		supportsReasoning: true,
		description: "High-speed version of M2.7 for low-latency scenarios",
	},
	"MiniMax-M2.5": {
		supportsTools: true,
		maxTokens: 128_000,
		contextWindow: 192_000,
		supportsImages: false,
		supportsPromptCache: true,
		supportsReasoning: true,
	},
	"MiniMax-M2.5-highspeed": {
		supportsTools: true,
		maxTokens: 128_000,
		contextWindow: 192_000,
		supportsImages: false,
		supportsPromptCache: true,
		supportsReasoning: true,
	},
	"MiniMax-M2.1": {
		supportsTools: true,
		maxTokens: 128_000,
		contextWindow: 192_000,
		supportsImages: false,
		supportsPromptCache: true,
	},
	"MiniMax-M2.1-lightning": {
		supportsTools: true,
		maxTokens: 128_000,
		contextWindow: 192_000,
		supportsImages: false,
		supportsPromptCache: true,
	},
	"MiniMax-M2": {
		supportsTools: true,
		maxTokens: 128_000,
		contextWindow: 192_000,
		supportsImages: false,
		supportsPromptCache: false,
	},
} as const satisfies Record<string, ModelInfo>

/**
 * Central registry of all hardcoded model maps.
 * This is used as the single source of truth for model-to-provider mapping.
 */
export const ALL_MODEL_MAPS: [ApiProvider, Record<string, ModelInfo>][] = [
	["anthropic", anthropicModels],
	["claude-code", claudeCodeModels],
	["deepseek", deepSeekModels],
	["huggingface", huggingFaceModels],
	["qwen", internationalQwenModels],
	["qwen", mainlandQwenModels],
	["mistral", mistralModels],
	["nebius", nebiusModels],
	["xai", xaiModels],
	["sambanova", sambanovaModels],
	["cerebras", cerebrasModels],
	["groq", groqModels],
	["moonshot", moonshotModels],
	["zai", internationalZAiModels],
	["zai", mainlandZAiModels],
	["fireworks", fireworksModels],
	["minimax", minimaxModels],
]

export const syntheticDefaultModelId = "hf:zai-org/GLM-4.7"
export const waferDefaultModelId = "wafer.ai/DeepSeek-V4-Pro"
export const veniceDefaultModelId = "venice-uncensored"
export const inferenceNetDefaultModelId = "google/gemma-3-27b-instruct/bf-16"
export const ovhcloudDefaultModelId = "gpt-oss-120b"
export const ollamaDefaultModelId = "llama3.2"
export const replicateDefaultModelId = "meta/llama-3-70b-instruct"

/**
 * Gets the provider for a given model ID based on hardcoded model maps.
 */
export function getProviderForModel(modelId: string): ApiProvider | undefined {
	for (const [provider, map] of ALL_MODEL_MAPS) {
		if (modelId in map) {
			return provider
		}
	}
	return undefined
}
