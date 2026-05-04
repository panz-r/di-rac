/**
 * Reusable Checkbox component for settings panels
 */

import { Box, Text } from "ink"
import React from "react"
import { COLORS } from "../constants/colors"

interface CheckboxProps {
	/** Label displayed next to the checkbox */
	label: string
	/** Current checked state */
	checked: boolean
	/** Whether this checkbox is currently selected/focused */
	isSelected?: boolean
	/** Whether this checkbox is disabled/inactive */
	disabled?: boolean
	/** Optional description shown below the label */
	description?: string
}

export const Checkbox: React.FC<CheckboxProps> = ({ label, checked, isSelected = false, disabled = false, description }) => {
	return (
		<Box flexDirection="column">
			<Text>
				<Text bold color={isSelected && !disabled ? COLORS.primaryBlue : undefined}>
					{isSelected && !disabled ? "❯" : " "}{" "}
				</Text>
				<Text color={disabled ? "gray" : (isSelected || checked ? COLORS.primaryBlue : "gray")}>{disabled ? "[ ]" : checked ? "[✓]" : "[ ]"}</Text>
				<Text color={disabled ? "gray" : (isSelected ? COLORS.primaryBlue : "white")}> {label}</Text>
				{isSelected && !disabled && <Text color="gray"> (Tab to toggle)</Text>}
			</Text>
			{description && (
				<Box marginLeft={6}>
					<Text color="gray">{description}</Text>
				</Box>
			)}
		</Box>
	)
}
