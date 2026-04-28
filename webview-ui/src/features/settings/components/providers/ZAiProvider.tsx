import { codingPlanZAiModelInfoSaneDefaults, internationalZAiModels, mainlandZAiModels, type ModelInfo } from "@shared/api"
import { Mode } from "@shared/ExtensionMessage"
import { VSCodeDropdown, VSCodeOption } from "@vscode/webview-ui-toolkit/react"
import { useMemo } from "react"
import { normalizeApiConfiguration } from "@/features/settings/components/utils/providerUtils"
import { useSettingsStore } from "@/features/settings/store/settingsStore"
import { DebouncedTextField } from "../common/DebouncedTextField"
import { ApiKeyField } from "../common/ApiKeyField"
import { ModelInfoView } from "../common/ModelInfoView"
import { DropdownContainer, ModelSelector } from "../common/ModelSelector"
import { useApiConfigurationHandlers } from "../utils/useApiConfigurationHandlers"

/**
 * Props for the ZAiProvider component
 */
interface ZAiProviderProps {
	showModelOptions: boolean
	isPopup?: boolean
	currentMode: Mode
}

/**
 * The Z AI provider configuration component
 */
export const ZAiProvider = ({ showModelOptions, isPopup, currentMode }: ZAiProviderProps) => {
	const { apiConfiguration } = useSettingsStore()
	const { handleFieldChange, handleModeFieldChange, handleModeFieldsChange } = useApiConfigurationHandlers()

	// Get the normalized configuration
	const { selectedModelId, selectedModelInfo } = normalizeApiConfiguration(apiConfiguration, currentMode)

	const isCodingPlan = apiConfiguration?.zaiApiLine === "coding-plan"

	// Determine which models to use based on API line selection
	const zaiModels = useMemo(
		() => (apiConfiguration?.zaiApiLine === "china" ? mainlandZAiModels : internationalZAiModels),
		[apiConfiguration?.zaiApiLine],
	)

	const handleCodingPlanModelChange = (newModelId: string) => {
		handleModeFieldsChange(
			{
				codingPlanZAiModelId: { plan: "planModeCodingPlanZAiModelId", act: "actModeCodingPlanZAiModelId" },
				codingPlanZAiModelInfo: { plan: "planModeCodingPlanZAiModelInfo", act: "actModeCodingPlanZAiModelInfo" },
			},
			{
				codingPlanZAiModelId: newModelId,
				codingPlanZAiModelInfo: codingPlanZAiModelInfoSaneDefaults,
			},
			currentMode,
		)
	}

	return (
		<div>
			<DropdownContainer className="dropdown-container" style={{ position: "inherit" }}>
				<label htmlFor="zai-entrypoint">
					<span style={{ fontWeight: 500, marginTop: 5 }}>Z AI Entrypoint</span>
				</label>
				<VSCodeDropdown
					id="zai-entrypoint"
					onChange={(e) => handleFieldChange("zaiApiLine", (e.target as any).value)}
					style={{
						minWidth: 130,
						position: "relative",
					}}
					value={apiConfiguration?.zaiApiLine || "international"}>
					<VSCodeOption value="international">api.z.ai</VSCodeOption>
					<VSCodeOption value="coding-plan">Coding Plan</VSCodeOption>
					<VSCodeOption value="china">open.bigmodel.cn</VSCodeOption>
				</VSCodeDropdown>
			</DropdownContainer>
			<p
				style={{
					fontSize: "12px",
					marginTop: 3,
					color: "var(--vscode-descriptionForeground)",
				}}>
				{isCodingPlan
					? "GLM Coding Plan subscription. No per-token charges."
					: "Please select the appropriate API entrypoint based on your location. If you are in China, choose open.bigmodel.cn. Otherwise, choose api.z.ai."}
			</p>
			<ApiKeyField
				initialValue={apiConfiguration?.zaiApiKey || ""}
				onChange={(value: string) => handleFieldChange("zaiApiKey", value)}
				providerName="Z AI"
				signupUrl={
					apiConfiguration?.zaiApiLine === "china"
						? "https://open.bigmodel.cn/console/overview"
						: "https://z.ai/manage-apikey/apikey-list"
				}
			/>

			{showModelOptions && (
				<>
					{isCodingPlan ? (
						<>
							<DebouncedTextField
								initialValue={selectedModelId || ""}
								onChange={handleCodingPlanModelChange}
								placeholder={"e.g. glm-5-turbo, glm-4.5-air"}
								style={{ width: "100%" }}
								type="text">
								<div className="flex items-center gap-2 mb-1">
									<span style={{ fontWeight: 500 }}>Model ID</span>
								</div>
							</DebouncedTextField>
							<ModelInfoView
								isPopup={isPopup}
								modelInfo={(selectedModelInfo as ModelInfo) || codingPlanZAiModelInfoSaneDefaults}
								selectedModelId={selectedModelId}
							/>
						</>
					) : (
						<>
							<ModelSelector
								label="Model"
								models={zaiModels}
								onChange={(e: any) =>
									handleModeFieldChange(
										{ plan: "planModeApiModelId", act: "actModeApiModelId" },
										e.target.value,
										currentMode,
									)
								}
								selectedModelId={selectedModelId}
							/>

							<ModelInfoView isPopup={isPopup} modelInfo={selectedModelInfo} selectedModelId={selectedModelId} />
						</>
					)}
				</>
			)}
		</div>
	)
}
