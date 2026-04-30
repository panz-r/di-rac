import { appendFile, readFile, rename } from "fs/promises"
import path from "path"
import { ensureTaskDirectoryExists } from "@core/storage/disk"
import type { ObservationEntry } from "./ObserverConfig"
import { Logger } from "@/shared/services/Logger"

export class ObservationStore {
	private cache: ObservationEntry[] = []
	private loaded = false
	private filePath: string | undefined

	constructor(private taskId: string) {}

	private async getFilePath(): Promise<string> {
		if (this.filePath) return this.filePath
		const taskDir = await ensureTaskDirectoryExists(this.taskId)
		this.filePath = path.join(taskDir, "observations.jsonl")
		return this.filePath
	}

	private async getArchivePath(): Promise<string> {
		const filePath = await this.getFilePath()
		return filePath.replace(".jsonl", "-archive.jsonl")
	}

	async append(entry: ObservationEntry): Promise<void> {
		const line = JSON.stringify(entry) + "\n"
		const filePath = await this.getFilePath()
		await appendFile(filePath, line, "utf8")
		this.cache.push(entry)
	}

	async load(): Promise<ObservationEntry[]> {
		if (this.loaded) return this.cache
		try {
			const filePath = await this.getFilePath()
			const content = await readFile(filePath, "utf8")
			this.cache = content
				.split("\n")
				.filter((line) => line.trim())
				.map((line) => {
					try {
						return JSON.parse(line) as ObservationEntry
					} catch {
						return null
					}
				})
				.filter((e): e is ObservationEntry => e !== null)
		} catch {
			this.cache = []
		}
		this.loaded = true
		return this.cache
	}

	getLatestObservation(): ObservationEntry | undefined {
		return this.cache.length > 0 ? this.cache[this.cache.length - 1] : undefined
	}

	getAllObservations(): ObservationEntry[] {
		return [...this.cache]
	}

	buildObservationBlock(): string {
		if (this.cache.length === 0) return ""
		return this.cache.map((e) => e.observationText).join("\n\n")
	}

	estimateTokenCount(): number {
		let totalChars = 0
		for (const entry of this.cache) {
			totalChars += entry.observationText.length
		}
		return Math.ceil(totalChars / 4)
	}

	async archiveAndReplace(newEntry: ObservationEntry): Promise<void> {
		// Move current observations to archive
		const filePath = await this.getFilePath()
		const archivePath = await this.getArchivePath()
		try {
			await rename(filePath, archivePath)
		} catch {
			// Archive may not exist yet — that's fine
		}
		// Write the new reflected entry as the sole observation
		const line = JSON.stringify(newEntry) + "\n"
		const { writeFile } = await import("fs/promises")
		await writeFile(filePath, line, "utf8")
		this.cache = [newEntry]
	}

	async dispose(): Promise<void> {
		this.cache = []
		this.loaded = false
		this.filePath = undefined
	}
}
