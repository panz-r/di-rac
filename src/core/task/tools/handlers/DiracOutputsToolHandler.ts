import type { ToolUse } from "@core/assistant-message"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import type { TaskConfig } from "../types/TaskConfig"
import { IToolHandler } from "../ToolExecutorCoordinator"

export class DiracOutputsToolHandler implements IToolHandler {
	readonly name = DiracDefaultTool.DIRAC_OUTPUTS

	getDescription(block: ToolUse): string {
		const file = block.params.file as string | undefined
		const clear = block.params.clear as boolean | undefined
		if (clear) return `[dirac_outputs: clear]`
		if (file) return `[dirac_outputs: read ${file}]`
		return `[dirac_outputs: list]`
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const om = config.services.outputManager
		const rawFile = block.params.file as string | undefined
		// Strip path prefixes so both "file.txt" and ".dirac/outputs/file.txt" work
		const file = rawFile
			? rawFile.replace(/^.dirac\/outputs\//, "").replace(/^.dirac\\outputs\\/, "")
			: undefined
		const clear = block.params.clear as boolean | undefined
		const list = block.params.list as boolean | undefined

		if (clear) {
			const count = om.clearOutputs()
			return `Cleared ${count} output file${count !== 1 ? "s" : ""}.`
		}

		if (file && !list) {
			const content = om.readOutput(file)
			if (!content) return `File not found: ${file}`
			return content
		}

		const outputs = om.listOutputs()
		if (outputs.length === 0) {
			return "No saved outputs. Outputs are saved automatically when tool results exceed 8KB."
		}

		const lines = outputs.map(
			(o) => `${o.filename.padEnd(45)} ${o.sizeKB.padStart(6)}KB  ${o.modified}`,
		)
		return `Saved outputs (${outputs.length}):\n\n${lines.join("\n")}\n\nUse dirac_outputs <filename> to read, or dirac_outputs --clear to delete all.`
	}
}
