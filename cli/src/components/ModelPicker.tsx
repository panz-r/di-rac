/**
 * Model picker component for model selection
 * Supports static model lists, dynamic loading via the API gateway, and custom text entry.
 * Tab on a model field opens this picker; Enter opens inline text editor.
 */

import { Box, Text } from "ink"
import Spinner from "ink-spinner"
import React, { useCallback, useEffect, useMemo, useState } from "react"
import { queryModels } from "@/core/api/providers/api-gateway"
import { refreshOpenRouterModels } from "@/core/controller/models/refreshOpenRouterModels"
import {
	type ApiProvider,
	anthropicDefaultModelId,
	anthropicModels,
	cerebrasDefaultModelId,
	cerebrasModels,
	claudeCodeDefaultModelId,
	claudeCodeModels,
	deepSeekDefaultModelId,
	deepSeekModels,
	fireworksDefaultModelId,
	fireworksModels,
	groqDefaultModelId,
	groqModels,
	huggingFaceDefaultModelId,
	huggingFaceModels,
	internationalQwenDefaultModelId,
	internationalQwenModels,
	internationalZAiDefaultModelId,
	internationalZAiModels,
	minimaxDefaultModelId,
	minimaxModels,
	mistralDefaultModelId,
	mistralModels,
	moonshotDefaultModelId,
	moonshotModels,
	nebiusDefaultModelId,
	nebiusModels,
	sambanovaDefaultModelId,
	sambanovaModels,
	xaiDefaultModelId,
	xaiModels,
} from "@/shared/api"
import { filterOpenRouterModelIds } from "@/shared/utils/model-filters"
import { COLORS } from "../constants/colors"
import { getOpenRouterDefaultModelId } from "../utils/openrouter-models"
import { SearchableList, SearchableListItem } from "./SearchableList"

export const CUSTOM_MODEL_ID = "__custom__"

export const providerModels: Record<string, { models: Record<string, unknown>; defaultId: string }> = {
	anthropic: { models: anthropicModels, defaultId: anthropicDefaultModelId },
	cerebras: { models: cerebrasModels, defaultId: cerebrasDefaultModelId },
	"claude-code": { models: claudeCodeModels, defaultId: claudeCodeDefaultModelId },
	deepseek: { models: deepSeekModels, defaultId: deepSeekDefaultModelId },
	fireworks: { models: fireworksModels, defaultId: fireworksDefaultModelId },
	groq: { models: groqModels, defaultId: groqDefaultModelId },
	huggingface: { models: huggingFaceModels, defaultId: huggingFaceDefaultModelId },
	minimax: { models: minimaxModels, defaultId: minimaxDefaultModelId },
	mistral: { models: mistralModels, defaultId: mistralDefaultModelId },
	moonshot: { models: moonshotModels, defaultId: moonshotDefaultModelId },
	nebius: { models: nebiusModels, defaultId: nebiusDefaultModelId },
	qwen: { models: internationalQwenModels, defaultId: internationalQwenDefaultModelId },
	sambanova: { models: sambanovaModels, defaultId: sambanovaDefaultModelId },
	xai: { models: xaiModels, defaultId: xaiDefaultModelId },
	zai: { models: internationalZAiModels, defaultId: internationalZAiDefaultModelId },
}

const gatewayModelsCache = new Map<string, string[]>()

export function hasStaticModels(provider: string): boolean {
	return provider in providerModels
}

export function hasModelPicker(provider: string): boolean {
	return hasStaticModels(provider) || provider === "openrouter" || provider === "opencode_go" || provider === "opencode_zen" || provider === "kilocode"
}

export function getDefaultModelId(provider: string): string {
	if (provider === "openrouter") {
		return getOpenRouterDefaultModelId()
	}
	return providerModels[provider]?.defaultId || ""
}

export function getModelList(provider: string): string[] {
	if (!hasStaticModels(provider)) return []
	return Object.keys(providerModels[provider].models)
}

interface ModelPickerProps {
	provider: string
	controller: any
	onChange: (modelId: string) => void
	onSubmit: (modelId: string) => void
	isActive?: boolean
}

export const ModelPicker: React.FC<ModelPickerProps> = ({ provider, controller, onChange, onSubmit, isActive = true }) => {
	const [isLoading, setIsLoading] = useState(false)
	const [dynamicModels, setDynamicModels] = useState<string[]>([])

	useEffect(() => {
		if (provider !== "openrouter" && provider !== "opencode_go" && provider !== "opencode_zen" && provider !== "kilocode") return

		const cached = gatewayModelsCache.get(provider)
		if (cached && cached.length > 0) {
			setDynamicModels(cached)
			return
		}

		let cancelled = false
		setIsLoading(true)

		const fetchModels = async () => {
			// Try gateway first
			try {
				const gwModels = await queryModels(provider)
				if (cancelled) return
				if (gwModels && gwModels.length > 0) {
					const ids = gwModels.map((m) => m.id).sort((a, b) => a.localeCompare(b))
					gatewayModelsCache.set(provider, ids)
					setDynamicModels(ids)
					return
				}
			} catch {
				// Gateway unavailable
			}

			if (cancelled) return

			// Fallback: TS-side fetch (openrouter only)
			if (provider === "openrouter") {
				try {
					const tsModels = await refreshOpenRouterModels(controller)
					if (cancelled) return
					if (tsModels) {
						const ids = Object.keys(tsModels).sort((a, b) => a.localeCompare(b))
						const filtered = filterOpenRouterModelIds(ids, provider as ApiProvider)
						if (filtered.length > 0) {
							gatewayModelsCache.set(provider, filtered)
						}
						setDynamicModels(filtered)
					}
				} catch {
					// Both paths failed
				}
			}
		}

		fetchModels().finally(() => {
			if (!cancelled) setIsLoading(false)
		})

		return () => { cancelled = true }
	}, [provider, controller])

	const modelList = useMemo(() => {
		if (provider === "openrouter" || provider === "opencode_go" || provider === "opencode_zen" || provider === "kilocode") {
			return dynamicModels
		}
		return getModelList(provider)
	}, [provider, dynamicModels])

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
				<Text color="gray"> Loading models...</Text>
			</Box>
		)
	}

	if (modelList.length === 0) {
		return (
			<Box flexDirection="column">
				<Text color="gray">No models available. Press Esc, then Enter to type a model ID.</Text>
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
