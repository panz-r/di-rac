/**
 * Model picker component for model selection.
 * All model discovery comes from the gateway.
 * Tab on a model field opens this picker; Enter opens inline text editor.
 */

import { Box, Text } from "ink"
import Spinner from "ink-spinner"
import React, { useCallback, useEffect, useMemo, useState } from "react"
import { queryModels } from "@/core/api/providers/api-gateway"
import { getProviderDefaultModelId } from "../utils/providers"
import { COLORS } from "../constants/colors"
import { SearchableList, SearchableListItem } from "./SearchableList"
import { Logger } from "@/shared/services/Logger"
import { StateManager } from "@/core/storage/StateManager"
import { ProviderToApiKeyMap } from "@/shared/storage"

export const CUSTOM_MODEL_ID = "__custom__"

const gatewayModelsCache = new Map<string, string[]>()

export function hasModelPicker(provider: string): boolean {
	return true
}

export function getDefaultModelId(provider: string): string {
	return getProviderDefaultModelId(provider)
}

interface ModelPickerProps {
	provider: string
	controller?: any
	onChange: (modelId: string) => void
	onSubmit: (modelId: string) => void
	isActive?: boolean
}

function getApiKeyForProvider(providerId: string): string | undefined {
	const stateManager = StateManager.get()
	const apiConfig = stateManager.getApiConfiguration()
	const keyField = ProviderToApiKeyMap[providerId as keyof typeof ProviderToApiKeyMap]
	if (!keyField) return undefined
	const fields = Array.isArray(keyField) ? keyField : [keyField]
	for (const field of fields) {
		const val = apiConfig[field as keyof typeof apiConfig] as string | undefined
		if (val) return val
	}
	return undefined
}

function getBaseUrlForProvider(providerId: string): string | undefined {
	const stateManager = StateManager.get()
	const apiConfig = stateManager.getApiConfiguration()
	const map: Record<string, string> = {
		openai: "openAiBaseUrl",
		anthropic: "anthropicBaseUrl",
		gemini: "geminiBaseUrl",
		openrouter: "openRouterBaseUrl",
	}
	const field = map[providerId]
	if (!field) return undefined
	return apiConfig[field as keyof typeof apiConfig] as string | undefined
}

export const ModelPicker: React.FC<ModelPickerProps> = ({ provider, controller, onChange, onSubmit, isActive = true }) => {
	const [isLoading, setIsLoading] = useState(true)
	const [gatewayModels, setGatewayModels] = useState<string[] | null>(null)
	const [diagnostic, setDiagnostic] = useState<string | null>(null)

	useEffect(() => {
		// Check cache first
		const cached = gatewayModelsCache.get(provider)
		if (cached && cached.length > 0) {
			setGatewayModels(cached)
			setIsLoading(false)
			setDiagnostic(null)
			return
		}

		let cancelled = false

		const fetchModels = async () => {
			try {
				const apiKey = getApiKeyForProvider(provider)
				const baseUrl = getBaseUrlForProvider(provider)
				const gwModels = await queryModels(provider, {
					api_key: apiKey,
					base_url: baseUrl,
				})
				if (cancelled) return
				if (gwModels && gwModels.length > 0) {
					const ids = gwModels.map((m) => m.id).sort((a, b) => a.localeCompare(b))
					gatewayModelsCache.set(provider, ids)
					setGatewayModels(ids)
					setDiagnostic(null)
					return
				}
				const detail = gwModels === null
					? "gateway returned null (socket error or no response)"
					: `gateway returned ${gwModels.length} models`
				setDiagnostic(`provider=${provider} key=${apiKey ? "set" : "none"} → ${detail}`)
				Logger.warn("[ModelPicker]", `queryModels("${provider}") returned ${gwModels === null ? "null" : gwModels.length + " models"}`)
			} catch (err: any) {
				if (cancelled) return
				const msg = err?.message || String(err)
				setDiagnostic(`provider=${provider}: ${msg}`)
				Logger.error("[ModelPicker]", `Failed to fetch models for "${provider}": ${msg}`)
			}

			if (!cancelled) {
				setGatewayModels([])
			}
		}

		fetchModels().finally(() => {
			if (!cancelled) setIsLoading(false)
		})

		return () => { cancelled = true }
	}, [provider])

	const modelList = useMemo(() => {
		if (gatewayModels && gatewayModels.length > 0) {
			return gatewayModels
		}
		return []
	}, [provider, gatewayModels])

	const items: SearchableListItem[] = useMemo(() => {
		return modelList.map((modelId) => ({
			id: modelId,
			label: modelId,
		}))
	}, [modelList])

	const handleSelect = useCallback((item: SearchableListItem) => {
		onChange(item.id)
		onSubmit(item.id)
	}, [onChange, onSubmit])

	if (isLoading) {
		return (
			<Box>
				<Text color={COLORS.primaryBlue}>
					<Spinner type="dots" />
				</Text>
				<Text color="gray"> Loading models for {provider}...</Text>
			</Box>
		)
	}

	if (modelList.length === 0) {
		return (
			<Box flexDirection="column">
				<Text color="gray">No models available.</Text>
				{diagnostic && <Text color="yellow">Debug: {diagnostic}</Text>}
				<Text color="gray">Press Esc, then Enter to type a model ID.</Text>
			</Box>
		)
	}

	return (
		<SearchableList
			isActive={isActive}
			items={items}
			onSelect={handleSelect}
		/>
	)
}
