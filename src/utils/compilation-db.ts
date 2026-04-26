import * as fs from "fs/promises"
import * as path from "path"
import { globby } from "globby"
import { Logger } from "@/shared/services/Logger"

export interface CompilationEntry {
	directory: string
	file: string
	command?: string
	arguments?: string[]
}

/**
 * Loader for compile_commands.json and compile_flags.txt, which provides precise build flags for C/C++ projects.
 * This is essential for resolving headers in complex build systems (Meson, CMake, Make).
 */
export class CompilationDatabaseLoader {
	private static instance: CompilationDatabaseLoader | null = null
	private entries: Map<string, CompilationEntry> = new Map()
	private includePaths: Set<string> = new Set()
	private isLoaded = false

	private constructor() {}

	static getInstance(): CompilationDatabaseLoader {
		if (!CompilationDatabaseLoader.instance) {
			CompilationDatabaseLoader.instance = new CompilationDatabaseLoader()
		}
		return CompilationDatabaseLoader.instance
	}

	async initialize(projectRoot: string): Promise<void> {
		if (this.isLoaded) return

		try {
			// 1. Find all compile_commands.json files in the project
			const dbPaths = await globby("**/compile_commands.json", {
				cwd: projectRoot,
				absolute: true,
				ignore: ["**/node_modules/**", "**/.git/**", "**/dist/**", "**/out/**"],
				deep: 5,
			})

			if (dbPaths.length === 0) {
				// Fallback to common locations if globby missed something (rare)
				const fallbackPaths = [
					path.join(projectRoot, "compile_commands.json"),
					path.join(projectRoot, "build", "compile_commands.json"),
					path.join(projectRoot, "builddir", "compile_commands.json"),
				]
				for (const p of fallbackPaths) {
					try {
						await fs.access(p)
						dbPaths.push(p)
					} catch {}
				}
			}

			for (const dbPath of dbPaths) {
				try {
					const content = await fs.readFile(dbPath, "utf8")
					const data = JSON.parse(content) as CompilationEntry[]
					this.parseEntries(data)
					Logger.info(`[CompilationDB] Loaded ${data.length} entries from ${dbPath}`)
				} catch (err) {
					Logger.warn(`[CompilationDB] Failed to parse ${dbPath}:`, err)
				}
			}

			// 2. Fallback to compile_flags.txt if no commands DB found or to supplement it
			const flagPaths = await globby("**/compile_flags.txt", {
				cwd: projectRoot,
				absolute: true,
				ignore: ["**/node_modules/**", "**/.git/**"],
				deep: 5,
			})

			for (const flagPath of flagPaths) {
				try {
					const content = await fs.readFile(flagPath, "utf8")
					const lines = content.split(/\r?\n/).map((l) => l.trim()).filter(Boolean)
					const dir = path.dirname(flagPath)
					this.parseFlags(lines, dir)
					Logger.info(`[CompilationDB] Loaded flags from ${flagPath}`)
				} catch (err) {
					Logger.warn(`[CompilationDB] Failed to parse ${flagPath}:`, err)
				}
			}

			if (dbPaths.length > 0 || flagPaths.length > 0) {
				this.isLoaded = true
			}
		} catch (error) {
			Logger.error("[CompilationDB] Initialization failed:", error)
		}
	}

	private parseEntries(entries: CompilationEntry[]): void {
		for (const entry of entries) {
			this.entries.set(path.normalize(entry.file), entry)
			const args = entry.arguments || entry.command?.split(/\s+/) || []
			this.parseFlags(args, entry.directory)
		}
	}

	private parseFlags(args: string[], baseDir: string): void {
		for (let i = 0; i < args.length; i++) {
			const arg = args[i]
			if (arg.startsWith("-I")) {
				let includePath = arg.slice(2)
				if (!includePath && i + 1 < args.length) {
					includePath = args[++i]
				}
				if (includePath) {
					this.includePaths.add(path.resolve(baseDir, includePath))
				}
			} else if (arg === "-isystem" && i + 1 < args.length) {
				this.includePaths.add(path.resolve(baseDir, args[++i]))
			}
		}
	}

	/**
	 * Resolves an include path (e.g., "my_header.h") using build-system flags.
	 */
	async resolveInclude(includeName: string, sourceFileDir: string): Promise<string | undefined> {
		// 1. Check relative to source file (standard C behavior for "quoted" includes)
		const relativePath = path.resolve(sourceFileDir, includeName)
		if (await this.fileExists(relativePath)) {
			return relativePath
		}

		// 2. Check all include paths from compile_commands.json
		for (const includePath of this.includePaths) {
			const resolvedPath = path.resolve(includePath, includeName)
			if (await this.fileExists(resolvedPath)) {
				return resolvedPath
			}
		}

		return undefined
	}

	private async fileExists(p: string): Promise<boolean> {
		try {
			await fs.access(p)
			return true
		} catch {
			return false
		}
	}

	dispose(): void {
		this.entries.clear()
		this.includePaths.clear()
		this.isLoaded = false
	}
}
