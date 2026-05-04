/**
 * Gateway client for sending provider configurations to the API Gateway
 */

import { StateManager } from "@/core/storage/StateManager"
import type { ApiProvider } from "@shared/api"
import { ALL_PROVIDERS } from "@shared/api"
import { ProviderToApiKeyMap, getProviderModelIdKey } from "@/shared/storage"
import net from "net"

function getGatewaySocketPath(): string {
	return process.env.DIRAC_API_GATEWAY_SOCKET || `${process.env.HOME}/.dirac/api-gateway.sock`
}

export interface ProviderConfig {
	api_key?: string
	base_url?: string
	model?: string
	region?: string
	project_id?: string
	timeout_ms?: number
	max_retries?: number
	supports_streaming?: boolean
	[key: string]: unknown
}

export interface SetProviderMessage {
	type: "set-provider"
	provider: string
	config: ProviderConfig
}

export interface GatewayResponse {
	id: number
	status: number
	body?: unknown
	error?: {
		code?: string
		message: string
		retriable?: boolean
	}
}

/**
 * Map provider → base URL field in ApiConfiguration
 */
const ProviderBaseUrlMap: Partial<Record<string, string>> = {
	anthropic: "anthropicBaseUrl",
	openai: "openAiBaseUrl",
	gemini: "geminiBaseUrl",
	lmstudio: "lmStudioBaseUrl",
	nvidiaNim: "nvidiaNimBaseUrl",
	"api-gateway": "apiGatewayBaseUrl",
}

/**
 * Get the API key field name(s) for a provider
 */
function getApiKeyField(providerId: string): string[] {
	const fields = ProviderToApiKeyMap[providerId as keyof typeof ProviderToApiKeyMap]
	if (!fields) return ["apiKey"]
	return Array.isArray(fields) ? fields : [fields]
}

/**
 * Extract provider configuration from StateManager for a specific provider
 */
function extractProviderConfig(stateManager: StateManager, providerId: string): ProviderConfig | null {
	const apiConfig = stateManager.getApiConfiguration()
	const modelKey = getProviderModelIdKey(providerId as ApiProvider, "act")
	const apiKeyFields = getApiKeyField(providerId)

	// Get API key from any of the provider's key fields
	let apiKey: string | undefined
	for (const field of apiKeyFields) {
		const key = apiConfig[field as keyof typeof apiConfig] as string | undefined
		if (key) {
			apiKey = key
			break
		}
	}

	// If no API key found and this isn't a provider that uses environment variables, skip
	if (!apiKey) {
		const hasCustomEndpoint = providerId === "openai" && apiConfig.openAiBaseUrl
		const noKeyNeeded = providerId === "lmstudio"

		if (!hasCustomEndpoint && !noKeyNeeded) {
			return null
		}
	}

	const config: ProviderConfig = {}

	// Set API key if available
	if (apiKey) {
		config.api_key = apiKey
	}

	// Set base URL for providers that support custom endpoints
	const baseUrlField = ProviderBaseUrlMap[providerId]
	if (baseUrlField) {
		const baseUrl = apiConfig[baseUrlField as keyof typeof apiConfig] as string | undefined
		if (baseUrl) {
			config.base_url = baseUrl
		}
	}

	// Set model ID
	if (modelKey) {
		const modelId = apiConfig[modelKey as keyof typeof apiConfig] as string | undefined
		if (modelId) {
			config.model = modelId
		}
	}

	// Set Azure API version
	if (providerId === "openai" && apiConfig.azureApiVersion) {
		config["azure_api_version"] = apiConfig.azureApiVersion
	}

	// Default timeout and retries
	config.timeout_ms = 30000
	config.max_retries = 3

	// Most providers support streaming
	config.supports_streaming = true

	return config
}

export async function sendProviderConfigsToGateway(): Promise<void> {
/**
 * Connect to the gateway socket and send provider configurations
 */

	return new Promise<void>((resolve, reject) => {
		const socket = net.createConnection(getGatewaySocketPath())

		socket.on("connect", async () => {
			console.log("[Gateway] Connected to API Gateway")
			const stateManager = StateManager.get()

			// Get all providers that are configured
			const configuredProviders: string[] = []

			// Check which providers have API keys set
			for (const providerId of ALL_PROVIDERS) {
				const config = extractProviderConfig(stateManager, providerId)
				if (config) {
					configuredProviders.push(providerId)
				}
			}

			if (configuredProviders.length === 0) {
				console.log("[Gateway] No configured providers found")
				socket.end()
				resolve()
				return
			}

			// Send set-provider message for each configured provider
			for (const providerId of configuredProviders) {
				const config = extractProviderConfig(stateManager, providerId)
				if (!config) continue

				const message: SetProviderMessage = {
					type: "set-provider",
					provider: providerId,
					config,
				}

				try {
					const response = await sendMessage(socket, message)
					if (response.status === 200) {
						console.log(`[Gateway] Sent config for provider: ${providerId}`)
					} else {
						console.error(`[Gateway] Failed to send config for ${providerId}: ${response.error?.message}`)
					}
				} catch (err) {
					console.error(`[Gateway] Error sending config for ${providerId}:`, err)
				}
			}

			socket.end()
			resolve()
		})

		socket.on("error", (err) => {
			// Socket doesn't exist yet - this is expected before gateway is started
			if (err.message.includes("ENOENT") || err.message.includes("connect ENOENT")) {
				console.log("[Gateway] API Gateway not yet running (socket not found)")
				resolve()
			} else {
				console.error("[Gateway] Socket error:", err)
				reject(err)
			}
		})

		socket.on("timeout", () => {
			socket.destroy()
			reject(new Error("[Gateway] Connection timeout"))
		})
	})
}

/**
 * Send a JSON message over the socket and wait for response
 */
function sendMessage(socket: net.Socket, message: SetProviderMessage): Promise<GatewayResponse> {
	return new Promise((resolve, reject) => {
		const timeout = setTimeout(() => {
			reject(new Error("Gateway request timeout"))
		}, 10000)

		const cleanup = () => {
			clearTimeout(timeout)
			socket.removeListener("data", onData)
			socket.removeListener("error", onError)
			socket.removeListener("close", onClose)
		}

		const onData = (data: Buffer) => {
			try {
				const response = JSON.parse(data.toString()) as GatewayResponse
				cleanup()
				resolve(response)
			} catch (err) {
				cleanup()
				reject(new Error(`Failed to parse gateway response: ${err}`))
			}
		}

		const onError = (err: Error) => {
			cleanup()
			reject(err)
		}

		const onClose = () => {
			cleanup()
			reject(new Error("Gateway connection closed unexpectedly"))
		}

		socket.addListener("data", onData)
		socket.addListener("error", onError)
		socket.addListener("close", onClose)

		socket.write(JSON.stringify(message) + "\n", (err) => {
			if (err) {
				cleanup()
				reject(err)
			}
		})
	})
}
