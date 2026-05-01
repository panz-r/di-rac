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

	async execute(config: TaskConfig, _block: ToolUse): Promise<ToolResponse> {
		const snapshotsDir = path.join(config.cwd, ".dirac-state", "snapshots")
		try {
			const entries = await fs.readdir(snapshotsDir)
			const turnDirs = entries
				.filter(e => e.startsWith("turn-"))
				.map(e => ({ name: e, num: parseInt(e.replace("turn-", ""), 10) }))
				.filter(e => !isNaN(e.num))
				.sort((a, b) => b.num - a.num)

			if (turnDirs.length === 0) {
				return "No turn snapshots found to undo."
			}

			const latest = turnDirs[0]
			const manifestPath = path.join(snapshotsDir, latest.name, `turn-${latest.num}.json`)
			const manifestRaw = await fs.readFile(manifestPath, "utf8")
			const manifest = JSON.parse(manifestRaw) as { files: Array<{ path: string; hash: string }> }

			const restored: string[] = []
			for (const entry of manifest.files) {
				const pathHash = createHash("sha256").update(entry.path).digest("hex").slice(0, 8)
				const snapshotPath = path.join(snapshotsDir, latest.name, `${pathHash}-${path.basename(entry.path)}`)
				try {
					await fs.copyFile(snapshotPath, entry.path)
					restored.push(entry.path)
				} catch {
					// skip files that can't be restored
				}
			}

			// Clean up the snapshot directory
			try {
				await fs.rm(path.join(snapshotsDir, latest.name), { recursive: true })
			} catch { /* ignore cleanup failure */ }

			return `Undid turn ${latest.num}. Restored: ${restored.join(", ") || "no files"}`
		} catch {
			return "No turn snapshots found to undo."
		}
	}
}
