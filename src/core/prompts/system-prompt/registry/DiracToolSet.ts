import { AgentConfigLoader } from "@core/task/tools/subagent/AgentConfigLoader"
import { DiracDefaultTool } from "@/shared/tools"
import { type DiracToolSpec, toolSpecInputSchema } from "../spec"
import { SystemPromptContext } from "../types"

export class DiracToolSet {
	private static tools: Map<string, DiracToolSet> = new Map()

	private constructor(
		public readonly id: string,
		public readonly config: DiracToolSpec,
	) {
		this._register()
	}

	public static register(config: DiracToolSpec): DiracToolSet {
		return new DiracToolSet(config.id, config)
	}

	private _register(): void {
		if (!DiracToolSet.tools.has(this.config.id)) {
			DiracToolSet.tools.set(this.config.id, this)
		}
	}

	public static getTools(): DiracToolSet[] {
		return Array.from(DiracToolSet.tools.values())
	}

	public static getToolByName(toolName: string): DiracToolSet | undefined {
		return DiracToolSet.tools.get(toolName)
	}

	public static getEnabledTools(context: SystemPromptContext): DiracToolSet[] {
		return Array.from(DiracToolSet.tools.values()).filter(
			(tool) => !tool.config.contextRequirements || tool.config.contextRequirements(context),
		)
	}

	private static getDynamicSubagentToolSpecs(context: SystemPromptContext): DiracToolSpec[] {
		if (context.subagentsEnabled !== true) {
			return []
		}

		const agentConfigs = AgentConfigLoader.getInstance().getAllCachedConfigsWithToolNames()
		return agentConfigs.map(({ toolName, config }) => ({
			id: DiracDefaultTool.USE_SUBAGENTS,
			name: toolName,
			description: `Use the "${config.name}" subagent: ${config.description}`,
			contextRequirements: (ctx) => ctx.subagentsEnabled === true,
			parameters: [
				{
					name: "prompt",
					required: true,
					instruction: "Helpful instruction for the task that the subagent will perform.",
				},
				{
					name: "timeout",
					required: false,
					instruction: "Optional timeout in seconds for the subagent.",
				},
				{
					name: "max_turns",
					required: false,
					instruction: "Optional maximum number of turns for the subagent.",
				},
			],
		}))
	}

	public static getEnabledToolSpecs(context: SystemPromptContext): DiracToolSpec[] {
		const registeredTools = DiracToolSet.getEnabledTools(context).map((tool) => tool.config)
		const dynamicSubagentTools = DiracToolSet.getDynamicSubagentToolSpecs(context)

		const includesDynamicSubagents = dynamicSubagentTools.length > 0
		const filteredRegistered = includesDynamicSubagents
			? registeredTools.filter((tool) => tool.id !== DiracDefaultTool.USE_SUBAGENTS)
			: registeredTools

		return [...filteredRegistered, ...dynamicSubagentTools]
	}

	/**
	 * Get the appropriate native tool converter for the given provider
	 */
	public static getNativeConverter(providerId: string, modelId?: string) {
		// All gateway-routed providers use input_schema format — the gateway handles conversion
		return toolSpecInputSchema
	}

	public static getNativeTools(context: SystemPromptContext) {

		// Base set
		const toolConfigs = DiracToolSet.getEnabledToolSpecs(context)

		const enabledTools = toolConfigs.filter(
			(tool) => typeof tool.description === "string" && tool.description.trim().length > 0,
		)
		const providerId = context.providerInfo?.providerId || "openai"
		const modelId = context.providerInfo?.model?.id
		const converter = DiracToolSet.getNativeConverter(providerId, modelId)

		return enabledTools.map((tool) => converter(tool, context))
	}
}
