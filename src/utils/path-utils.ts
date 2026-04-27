import * as path from "node:path"
import * as fs from "node:fs"

export class PathEscapeError extends Error {
	constructor(public readonly rawPath: string) {
		super(`Path escape detected: '${rawPath}' is outside the workspace root.`)
		this.name = "PathEscapeError"
	}
}

/**
 * Normalizes a path to be project-relative.
 * Ensures the path stays within the workspace root.
 * 
 * @param raw - The raw path provided (absolute or relative)
 * @param workspaceRoot - The absolute path to the workspace root
 * @returns The normalized path relative to the workspace root
 * @throws PathEscapeError if the path is outside the workspace root
 */
export function normalizePath(raw: string, workspaceRoot: string): string {
	// 1. Resolve absolute or relative
	const resolved = path.resolve(workspaceRoot, raw)

	// 2. Resolve symlinks if the file exists
	let realPath = resolved
	try {
		realPath = fs.realpathSync(resolved)
	} catch (e) {
		// File might not exist yet (e.g. write_to_file), that's fine.
		// We still have the path.resolve'd version which handles .. segments.
	}

	// 3. Ensure it stays inside workspace
	// Standardizing for comparison: Ensure both are absolute and standardized
	const absoluteRoot = fs.realpathSync(path.resolve(workspaceRoot))
	
	const relative = path.relative(absoluteRoot, realPath)
	
	if (relative.startsWith("..") || path.isAbsolute(relative)) {
		throw new PathEscapeError(raw)
	}

	// 4. Return relative to workspace root (using forward slashes)
	return relative.replace(/\\/g, "/") || "."
}
