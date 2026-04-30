import type { ApiProviderInfo } from "@core/api"
import { DiracRulesToggles } from "@shared/dirac-rules"
import fs from "fs/promises"
import { telemetryService } from "@/services/telemetry"
import { Logger } from "@/shared/services/Logger"
import {
	condenseToolResponse,
	explainChangesToolResponse,
	newRuleToolResponse,
	newTaskToolResponse,
	reportBugToolResponse,
} from "../prompts/commands"
import { getSkillContent } from "../context/instructions/user-instructions/skills"
import { SkillMetadata } from "@/shared/skills"

type FileBasedWorkflow = {
	fullPath: string
	fileName: string
	isRemote: false
}

// TODO: Remote workflows are not yet wired to a config source.
// When remote workflow loading is implemented, add an `isRemote: true`
// variant here and populate from the remote config loader.
type Workflow = FileBasedWorkflow

/**
 * Processes text for slash commands and transforms them with appropriate instructions
 * This is called after parseMentions() to process any slash commands in the user's message
 */
export async function parseSlashCommands(
	text: string,
	localWorkflowToggles: DiracRulesToggles,
	globalWorkflowToggles: DiracRulesToggles,
	ulid: string,
	providerInfo?: ApiProviderInfo,
	availableSkills: SkillMetadata[] = [],
	remoteWorkflowToggles: Record<string, boolean> = {},
): Promise<{ processedText: string; needsDiracrulesFileCheck: boolean }> {
	const SUPPORTED_DEFAULT_COMMANDS = ["newtask", "smol", "compact", "newrule", "reportbug", "explain-changes"]

	const commandReplacements: Record<string, string> = {
		newtask: newTaskToolResponse(),
		smol: condenseToolResponse(),
		compact: condenseToolResponse(),
		newrule: newRuleToolResponse(),
		reportbug: reportBugToolResponse(),
		"explain-changes": explainChangesToolResponse(),
	}

	// Regex patterns to extract content from different XML tags
	const tagPatterns = [
		{ tag: "task", regex: /<task>([\s\S]*?)<\/task>/i },
		{ tag: "feedback", regex: /<feedback>([\s\S]*?)<\/feedback>/i },
		{ tag: "answer", regex: /<answer>([\s\S]*?)<\/answer>/i },
		{ tag: "user_message", regex: /<user_message>([\s\S]*?)<\/user_message>/i },
	]

	// Regex to find slash commands anywhere in text (not just at the beginning).
	// This mirrors how @ mentions work - they can appear anywhere in a message.
	//
	// Pattern breakdown: /(^|\s)\/([a-zA-Z0-9_.:@-]+)(?=\s|$)/
	//   - (^|\s)  : Must be at start of string OR preceded by whitespace
	//   - \/      : The literal slash character
	//   - ([a-zA-Z0-9_.:@-]+) : The command name (letters, numbers, underscore, dot, hyphen, colon, @)
	//   - (?=\s|$): Must be followed by whitespace or end of string (lookahead)
	//
	// This safely avoids false matches in:
	//   - URLs: "http://example.com/newtask" - slash not preceded by whitespace
	//   - File paths: "some/path/newtask" - same reason
	//   - Partial words: "foo/bar" - same reason
	//
	// Only ONE slash command per message is processed (first match found).
	const slashCommandInTextRegex = /(^|\s)\/([a-zA-Z0-9_.:@-]+)(?=\s|$)/

	// Helper function to calculate positions and remove slash command from text
	const removeSlashCommand = (
		fullText: string,
		_tagContent: string,
		contentStartIndex: number,
		slashMatch: RegExpExecArray,
	): string => {
		// slashMatch[1] is the whitespace or empty string before the slash
		// Remove the preceding whitespace along with the command, unless at string start
		const removeStart = slashMatch.index + (slashMatch[1] ? slashMatch[1].length : 0)
		const slashPositionInFullText = contentStartIndex + removeStart
		const commandText = "/" + slashMatch[2]
		const commandEndPosition = slashPositionInFullText + commandText.length

		return fullText.substring(0, contentStartIndex + slashMatch.index) + fullText.substring(commandEndPosition)
	}

	// if we find a valid match, we will return inside that block
	for (const { regex } of tagPatterns) {
		const tagMatch = regex.exec(text)

		if (tagMatch) {
			const tagContent = tagMatch[1]
			const tagStartIndex = tagMatch.index
			const contentStartIndex = text.indexOf(tagContent, tagStartIndex)

			// Find slash command within the tag content
			const slashMatch = slashCommandInTextRegex.exec(tagContent)

			if (!slashMatch) {
				continue
			}

			// slashMatch[1] is the whitespace or empty string before the slash
			// slashMatch[2] is the command name
			const commandName = slashMatch[2] // casing matters

			// we give preference to the default commands if the user has a file with the same name
			if (SUPPORTED_DEFAULT_COMMANDS.includes(commandName)) {
				// remove the slash command and add custom instructions at the top of this message
				const textWithoutSlashCommand = removeSlashCommand(text, tagContent, contentStartIndex, slashMatch)
				const processedText = commandReplacements[commandName] + textWithoutSlashCommand

				// Track telemetry for builtin slash command usage
				telemetryService.captureSlashCommandUsed(ulid, commandName, "builtin")

				return { processedText: processedText, needsDiracrulesFileCheck: commandName === "newrule" }
			}

			const globalWorkflows: Workflow[] = Object.entries(globalWorkflowToggles)
				.filter(([_, enabled]) => enabled)
				.map(([filePath, _]) => ({
					fullPath: filePath,
					fileName: filePath.replace(/^.*[/\\]/, ""),
					isRemote: false,
				}))

			const localWorkflows: Workflow[] = Object.entries(localWorkflowToggles)
				.filter(([_, enabled]) => enabled)
				.map(([filePath, _]) => ({
					fullPath: filePath,
					fileName: filePath.replace(/^.*[/\\]/, ""),
					isRemote: false,
				}))

			// local workflows have precedence over global workflows
			// TODO: Add remote workflows once the remote config loader is implemented
			const enabledWorkflows: Workflow[] = [...localWorkflows, ...globalWorkflows]

			// Then check if the command matches any enabled workflow filename
			const matchingWorkflow = enabledWorkflows.find((workflow) => workflow.fileName === commandName)

			if (matchingWorkflow) {
				try {
					const workflowContent = (await fs.readFile(matchingWorkflow.fullPath, "utf8")).trim()

					// remove the slash command and add custom instructions at the top of this message
					const textWithoutSlashCommand = removeSlashCommand(text, tagContent, contentStartIndex, slashMatch)
					const processedText =
						`<explicit_instructions type="${matchingWorkflow.fileName}">\n${workflowContent}\n</explicit_instructions>\n` +
						textWithoutSlashCommand

					// Track telemetry for workflow command usage
					telemetryService.captureSlashCommandUsed(ulid, commandName, "workflow")

					return { processedText, needsDiracrulesFileCheck: false }
				} catch (error) {
					Logger.error(`Error reading workflow file ${matchingWorkflow.fullPath}: ${error}`)
				}
			}

			// Check if the command matches any enabled skill
			const matchingSkill = availableSkills.find((skill) => skill.name === commandName)
			if (matchingSkill) {
				try {
					const skillContent = await getSkillContent(matchingSkill.name, availableSkills)
					if (skillContent) {
						// remove the slash command and add custom instructions at the top of this message
						const textWithoutSlashCommand = removeSlashCommand(text, tagContent, contentStartIndex, slashMatch)
						let processedText = `<explicit_instructions type="skill" name="${skillContent.name}">\n${skillContent.instructions}\n</explicit_instructions>\n`

						// Check if the content of the tag we matched is now empty
						const newTagContentMatch = regex.exec(textWithoutSlashCommand)
						const newTagContent = newTagContentMatch ? newTagContentMatch[1].trim() : ""

						if (!newTagContent) {
							processedText += `\n(Note: The user has explicitly activated the "${skillContent.name}" skill via a slash command. This skill is now active. Please acknowledge its activation, summarize how you can help based on its instructions, and ask the user for the specific target or task they want you to perform, or propose a first step if appropriate.)\n`
						}

						processedText += textWithoutSlashCommand

						// Track telemetry for skill command usage
						telemetryService.captureSlashCommandUsed(ulid, commandName, "skill")

						return { processedText, needsDiracrulesFileCheck: false }
					}
				} catch (error) {
					Logger.error(`Error loading skill ${matchingSkill.name}: ${error}`)
				}
			}
		}
	}

	// if no supported commands are found, return the original text
	return { processedText: text, needsDiracrulesFileCheck: false }
}
