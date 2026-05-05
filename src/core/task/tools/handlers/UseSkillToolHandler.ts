import type { ToolUse } from "@core/assistant-message"
import {
	getOrDiscoverSkills,
	getSkillContent,
	listSupportingFiles
} from "@core/context/instructions/user-instructions/skills"
import type { SkillMetadata } from "@shared/skills"
import { telemetryService } from "@/services/telemetry"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import type { IPartialBlockHandler, IToolHandler } from "../ToolExecutorCoordinator"
import type { TaskConfig } from "../types/TaskConfig"
import type { StronglyTypedUIHelpers } from "../types/UIHelpers"

export class UseSkillToolHandler implements IToolHandler, IPartialBlockHandler {
	readonly name = DiracDefaultTool.USE_SKILL

	constructor() {}

	getDescription(block: ToolUse): string {
		const skillName = block.params.skill_name
		return skillName ? `[${block.name} for "${skillName}"]` : `[${block.name}]`
	}

	async handlePartialBlock(block: ToolUse, uiHelpers: StronglyTypedUIHelpers): Promise<void> {
		const skillName = block.params.skill_name
		if (uiHelpers.getConfig().isSubagentExecution || !skillName) {
			return
		}
		const message = JSON.stringify({ tool: "useSkill", path: skillName || "" })

		await uiHelpers.say("tool", message, undefined, undefined, true)
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const skillName: string | undefined = block.params.skill_name

		if (!skillName) {
			config.taskState.consecutiveMistakeCount++
			return `Error: Missing required parameter 'skill_name'. Please provide the name of the skill to activate.`
		}

		// Discover skills on-demand (lazy loading)
		const resolvedSkills = await getOrDiscoverSkills(config.cwd, config.taskState)

		// Filter by toggle state
		const stateManager = config.services.stateManager
		const globalSkillsToggles = stateManager.getGlobalSettingsKey("globalSkillsToggles") ?? {}
		const localSkillsToggles = stateManager.getWorkspaceStateKey("localSkillsToggles") ?? {}
		const availableSkills = resolvedSkills.filter((skill) => {
			const toggles = skill.source === "global" ? globalSkillsToggles : localSkillsToggles
			return toggles[skill.path] !== false
		})

		if (availableSkills.length === 0) {
			return `Error: No skills are available. Skills may be disabled or not configured.`
		}

		const globalCount = availableSkills.filter((skill) => skill.source === "global").length
		const projectCount = availableSkills.filter((skill) => skill.source === "project").length

		const apiConfig = config.services.stateManager.getApiConfiguration()
		const currentMode = config.services.stateManager.getGlobalSettingsKey("mode")
		const provider = currentMode === "plan" ? apiConfig.planModeApiProvider : apiConfig.actModeApiProvider

		// Show tool message
		const message = JSON.stringify({ tool: "useSkill", path: skillName })
		if (!config.isSubagentExecution) {
			await config.callbacks.removeLastPartialMessageIfExistsWithType("say", "tool")
			await config.callbacks.say("tool", message, undefined, undefined, false)
		}

		config.taskState.consecutiveMistakeCount = 0

		try {
			const skillContent = await getSkillContent(skillName, availableSkills)

			if (!skillContent) {
				const availableNames = availableSkills.map((s: SkillMetadata) => s.name).join(", ")
				return `Error: Skill "${skillName}" not found. Available skills: ${availableNames || "none"}`
			}

			telemetryService.safeCapture(
				() =>
					telemetryService.captureSkillUsed({
						ulid: config.ulid,
						skillName,
						skillSource: skillContent.source === "global" ? "global" : "project",
						skillsAvailableGlobal: globalCount,
						skillsAvailableProject: projectCount,
						provider,
						modelId: config.api.getModel().id,
					}),
				"UseSkillToolHandler.execute",
			)

			telemetryService.captureToolUsage(
				config.ulid,
				this.name,
				config.api.getModel().id,
				provider as string,

				true, // autoApproved - use_skill is auto-approved
				true,
				undefined,
				block.isNativeToolCall,

			)

			const { docs, scripts } = await listSupportingFiles(skillContent.path)
			let activationMessage = `# Skill "${skillContent.name}" is now active\n\n${skillContent.instructions}\n\n---\n`
			activationMessage += `IMPORTANT: The skill is now loaded. Do NOT call use_skill again for this task. Simply follow the instructions above to complete the user's request.\n`

			const skillDir = skillContent.path.replace(/SKILL\.md$/, "")
			if (docs.length > 0 || scripts.length > 0) {
				activationMessage += `\nYou may access supporting files in the skill directory: ${skillDir}\n`
				if (docs.length > 0) {
					activationMessage += `\nDocumentation available:\n${docs.map((f) => `- ${skillDir}docs/${f}`).join("\n")}\n`
				}
				if (scripts.length > 0) {
					activationMessage += `\nScripts available (run via bash):\n${scripts.map((f) => `- ${skillDir}scripts/${f}`).join("\n")}\n`
				}
			} else {
				activationMessage += `\nYou may access other files in the skill directory at: ${skillDir}`
			}

			return activationMessage
		} catch (error) {
			return `Error loading skill "${skillName}": ${(error as Error)?.message}`
		}
	}
}
