import getFolderSize from "get-folder-size"
import path from "path"
import { HostProvider } from "@/hosts/host-provider"
import { Logger } from "@/shared/services/Logger"

/**
 * Gets the total size of tasks directory
 * @returns The total size in bytes, or null if calculation fails
 */
export async function getTotalTasksSize(): Promise<number | null> {
	const tasksDir = path.resolve(HostProvider.get().globalStorageFsPath, "tasks")

	try {
		return await getFolderSize.loose(tasksDir)
	} catch (error) {
		Logger.error("Failed to calculate total task size:", error)
		return null
	}
}
