/**
 * Provider picker component for API provider selection
 */

import React, { useMemo } from "react"
import { StateManager } from "@/core/storage/StateManager"
import { isProviderConfigured } from "@/shared/providers/provider-registry"
import { getProviderLabel, useValidProviders } from "../utils/providers"
import { SearchableList, type SearchableListItem } from "./SearchableList"

// Re-export for backwards compatibility
export { getProviderLabel }

interface ProviderPickerProps {
	onSelect: (providerId: string) => void
	onEdit?: (providerId: string) => void
	isActive?: boolean
}

export const ProviderPicker: React.FC<ProviderPickerProps> = ({ onSelect, onEdit, isActive = true }) => {
	// Get API configuration to check which providers are configured
	const apiConfig = StateManager.get().getApiConfiguration()
	const sorted = useValidProviders()

	const items: SearchableListItem[] = useMemo(() => {
		return sorted.map((providerId: string) => ({
			id: providerId,
			label: getProviderLabel(providerId),
			suffix: isProviderConfigured(providerId, apiConfig) ? "(Configured)" : undefined,
		}))
	}, [apiConfig, sorted])

	return <SearchableList isActive={isActive} items={items} onSelect={(item) => onSelect(item.id)} onEdit={onEdit ? (item) => onEdit(item.id) : undefined} />
}
