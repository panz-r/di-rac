import type { ToolUse } from "@core/assistant-message"
import { DiracDefaultTool } from "@/shared/tools"
import type { ToolResponse } from "../../index"
import type { IToolHandler } from "../ToolExecutorCoordinator"
import type { TaskConfig } from "../types/TaskConfig"
import { createHash } from "crypto"
import * as path from "path"
import * as fs from "fs/promises"

export class DiracUndoToolHandler implements IToolHandler {
	readonly name = DiracDefaultTool.DIRAC_UNDO

	getDescription(_block: ToolUse): string {
		return `[${this.name}]`
	}

	async execute(config: TaskConfig, block: ToolUse): Promise<ToolResponse> {
		const snapshotsDir = path.join(config.cwd, ".dirac-state", "snapshots")
		const turnNum = block.params?.turn ? parseInt(block.params.turn, 10) : undefined

		try {
			const entries = await fs.readdir(snapshotsDir)
			const turnDirs = entries
				.filter(e => e.startsWith("turn-"))
				.map(e => ({ name: e, num: parseInt(e.replace("turn-", ""), 10) }))
				.filter(e => !isNaN(e.num))
				.sort((a, b) => b.num - a.num)

			if (turnDirs.length === 0) {
				return "No turn snapshots found."
			}

			// List mode: no turn specified
			if (turnNum === undefined || isNaN(turnNum)) {
				const listing = turnDirs
					.slice(0, 10)
					.map(t => `  turn-${t.num}`)
					.join("\n")
				return `Available snapshots (newest first):\n${listing}\nUse --turn N to restore a specific snapshot.`
			}

			// Restore specific turn
			const target = turnDirs.find(t => t.num === turnNum)
			if (!target) {
				return `Snapshot turn-${turnNum} not found. Available: ${turnDirs.map(t => t.num).join(", ")}`
			}

			const manifestPath = path.join(snapshotsDir, target.name, `turn-${target.num}.json`)
			const manifestRaw = await fs.readFile(manifestPath, "utf8")
			const manifest = JSON.parse(manifestRaw) as { files: Array<{ path: string; hash: string }> }

			const restored: string[] = []
			for (const entry of manifest.files) {
				const pathHash = createHash("sha256").update(entry.path).digest("hex").slice(0, 8)
				const snapshotPath = path.join(snapshotsDir, target.name, `${pathHash}-${path.basename(entry.path)}`)
				try {
					await fs.copyFile(snapshotPath, entry.path)
					restored.push(entry.path)
				} catch {
					// skip files that can't be restored
				}
			}

			return `Restored turn ${target.num}. Files: ${restored.join(", ") || "no files restored"}`
		} catch {
			return "No turn snapshots found."
		}
	}
}
