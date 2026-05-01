import type { ToolUse } from "@core/assistant-message"
import { DiracToolSet } from "@core/prompts/system-prompt/registry/DiracToolSet"
import type { DiracToolSpec } from "@core/prompts/system-prompt/spec"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import { IToolHandler } from "../ToolExecutorCoordinator"

export class ToolSearchToolHandler implements IToolHandler {
	readonly name = DiracDefaultTool.TOOL_SEARCH

	getDescription(block: ToolUse): string {
		return `[tool_search: ${block.params.query ?? "list all"}]`
	}

	async execute(_config: any, block: ToolUse): Promise<ToolResponse> {
		const query = block.params.query as string | undefined

		const allSpecs = this.getAllSpecs()

		if (query) return this.searchTools(allSpecs, query)
		return this.listAllTools(allSpecs)
	}

	private getAllSpecs(): DiracToolSpec[] {
		return DiracToolSet.getTools().map((t) => t.config)
	}

	private listAllTools(specs: DiracToolSpec[]): string {
		const lines = specs.map((spec) => {
			const brief = spec.metadata?.llmsBrief ?? spec.description.split("\n")[0]
			return `${spec.name.padEnd(22)}— ${brief}`
		})
		return `Available tools (${specs.length}):\n\n${lines.join("\n")}\n\nUse tool_search <query> to search by keyword.`
	}

	private searchTools(specs: DiracToolSpec[], query: string): string {
		const q = query.toLowerCase()
		const matches = specs.filter((spec) => {
			const name = spec.name.toLowerCase()
			const desc = spec.description.toLowerCase()
			const tags = (spec.metadata?.tags ?? []).join(" ").toLowerCase()
			const category = (spec.metadata?.category ?? "").toLowerCase()
			return name.includes(q) || desc.includes(q) || tags.includes(q) || category.includes(q)
		})

		if (matches.length === 0) {
			return `No tools found matching "${query}". Use tool_search (no args) to list all tools.`
		}

		const lines = matches.map((spec) => {
			const brief = spec.metadata?.llmsBrief ?? spec.description.split("\n")[0]
			return `${spec.name.padEnd(22)}— ${brief}`
		})
		return `Found ${matches.length} tool${matches.length > 1 ? "s" : ""}:\n\n${lines.join("\n")}`
	}
}
