import * as fs from "fs"
import * as path from "path"
import type { Database } from "sql.js"
import initSqlJs from "sql.js"
import { Logger } from "../../shared/services/Logger"
import { SymbolLocation } from "./SymbolIndexService"
import { fileExistsAtPath } from "@utils/fs"

export interface FileMetadata {
	mtime: number
	size: number
}

export class SymbolIndexDatabase {
	private db: Database
	private dbPath: string
	private isDirty = false

	// Cached prepared statements for high-performance loops
	private insertNameStmt: any | null = null
	private getNameIdStmt: any | null = null
	private insertKindStmt: any | null = null
	private getKindIdStmt: any | null = null
	private insertSymbolStmt: any | null = null

	private constructor(db: Database, dbPath: string) {
		this.db = db
		this.dbPath = dbPath
	}

	public static async create(dbPath: string): Promise<SymbolIndexDatabase> {
		Logger.info(`[SymbolIndexDatabase] Initializing database at ${dbPath}`)
		const dbDir = path.dirname(dbPath)
		if (!(await fileExistsAtPath(dbDir))) {
			Logger.info(`[SymbolIndexDatabase] Creating database directory: ${dbDir}`)
			await fs.promises.mkdir(dbDir, { recursive: true })
		}

		const SQL = await initSqlJs({
			locateFile: (file) => path.join(__dirname, file),
		})
		let db: Database

		try {
			if (await fileExistsAtPath(dbPath)) {
				Logger.info(`[SymbolIndexDatabase] Loading existing database from ${dbPath}`)
				const fileBuffer = await fs.promises.readFile(dbPath)
				db = new SQL.Database(fileBuffer)
			} else {
				Logger.info(`[SymbolIndexDatabase] Creating new database`)
				db = new SQL.Database()
			}
		} catch (error) {
			Logger.error(`[SymbolIndexDatabase] Failed to load database, creating new:`, error)
			db = new SQL.Database()
		}

		const instance = new SymbolIndexDatabase(db, dbPath)
		await instance.initialize()
		return instance
	}

	private async initialize(): Promise<void> {
		Logger.info("[SymbolIndexDatabase] Running schema initialization")
		this.db.run("PRAGMA foreign_keys = ON")
		this.db.run("PRAGMA page_size = 4096")
		this.db.run("PRAGMA cache_size = -2000") // 2MB cache
		this.db.run("PRAGMA journal_mode = MEMORY") // Speed up transactions in WASM

		// Create files table
		this.db.run(`
			CREATE TABLE IF NOT EXISTS files (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				path TEXT UNIQUE NOT NULL,
				mtime INTEGER NOT NULL,
				size INTEGER NOT NULL
			);
		`)

		// Create names table
		this.db.run(`
			CREATE TABLE IF NOT EXISTS names (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				text TEXT UNIQUE NOT NULL
			);
		`)

		// Create kinds table
		this.db.run(`
			CREATE TABLE IF NOT EXISTS kinds (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				text TEXT UNIQUE NOT NULL
			);
		`)

		// Create symbols table
		this.db.run(`
			CREATE TABLE IF NOT EXISTS symbols (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				file_id INTEGER NOT NULL,
				name_id INTEGER NOT NULL,
				type TEXT NOT NULL,
				kind_id INTEGER,
				start_line INTEGER NOT NULL,
				start_column INTEGER NOT NULL,
				end_line INTEGER NOT NULL,
				end_column INTEGER NOT NULL,
				FOREIGN KEY (file_id) REFERENCES files(id) ON DELETE CASCADE,
				FOREIGN KEY (name_id) REFERENCES names(id) ON DELETE CASCADE,
				FOREIGN KEY (kind_id) REFERENCES kinds(id) ON DELETE SET NULL
			);
		`)

		// Create dependencies table for includes/imports
		this.db.run(`
			CREATE TABLE IF NOT EXISTS dependencies (
				file_id INTEGER NOT NULL,
				dependency_id INTEGER NOT NULL,
				FOREIGN KEY (file_id) REFERENCES files(id) ON DELETE CASCADE,
				FOREIGN KEY (dependency_id) REFERENCES files(id) ON DELETE CASCADE,
				PRIMARY KEY (file_id, dependency_id)
			);
		`)

		this.db.run("CREATE INDEX IF NOT EXISTS idx_symbols_name_id ON symbols(name_id)")
		this.db.run("CREATE INDEX IF NOT EXISTS idx_symbols_name_type ON symbols(name_id, type)")
		this.db.run("CREATE INDEX IF NOT EXISTS idx_symbols_file_id ON symbols(file_id)")
		this.db.run("CREATE INDEX IF NOT EXISTS idx_dependencies_file_id ON dependencies(file_id)")

		// Prepare reusable statements
		this.insertNameStmt = this.db.prepare("INSERT OR IGNORE INTO names (text) VALUES (?)")
		this.getNameIdStmt = this.db.prepare("SELECT id FROM names WHERE text = ?")
		this.insertKindStmt = this.db.prepare("INSERT OR IGNORE INTO kinds (text) VALUES (?)")
		this.getKindIdStmt = this.db.prepare("SELECT id FROM kinds WHERE text = ?")
		this.insertSymbolStmt = this.db.prepare(`
			INSERT INTO symbols (file_id, name_id, type, kind_id, start_line, start_column, end_line, end_column)
			VALUES (?, ?, ?, ?, ?, ?, ?, ?)
		`)

		// Perform migration if needed (detect old schema)
		try {
			const checkStmt = this.db.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='dependencies'")
			const hasDependencies = checkStmt.step()
			checkStmt.free()

			if (!hasDependencies) {
				Logger.info("[SymbolIndexDatabase] Migrating database to normalized schema (v4)")
				this.db.run("DROP TABLE IF EXISTS symbols")
				this.db.run("DROP TABLE IF EXISTS files")
				this.db.run("DROP TABLE IF EXISTS names")
				this.db.run("DROP TABLE IF EXISTS kinds")
				await this.initialize() // Re-run to create new tables
				return
			}
		} catch (error) {
			Logger.warn("[SymbolIndexDatabase] Migration check failed, ignoring:", error)
		}

		Logger.info("[SymbolIndexDatabase] Schema initialization complete")
	}

	public async save(): Promise<void> {
		if (!this.isDirty) {
			return
		}
		try {
			Logger.info(`[SymbolIndexDatabase] Saving database to ${this.dbPath}`)
			const data = this.db.export()
			const buffer = Buffer.from(data)
			await fs.promises.writeFile(this.dbPath, buffer)
			this.isDirty = false
			Logger.info(`[SymbolIndexDatabase] Database saved successfully`)
		} catch (error) {
			Logger.error(`[SymbolIndexDatabase] Failed to save database:`, error)
		}
	}

	public getFileMetadata(relPath: string): FileMetadata | null {
		const stmt = this.db.prepare("SELECT mtime, size FROM files WHERE path = ?")
		stmt.bind([relPath])
		if (stmt.step()) {
			const result = stmt.getAsObject() as any
			stmt.free()
			return { mtime: result.mtime, size: result.size }
		}
		stmt.free()
		return null
	}

	public getAllFilesMetadata(): Map<string, FileMetadata> {
		const stmt = this.db.prepare("SELECT path, mtime, size FROM files")
		const map = new Map<string, FileMetadata>()
		while (stmt.step()) {
			const row = stmt.getAsObject() as any
			map.set(row.path, { mtime: row.mtime, size: row.size })
		}
		stmt.free()
		return map
	}

	private getOrCreateId(table: "names" | "kinds", text: string): number {
		const insertStmt = table === "names" ? this.insertNameStmt : this.insertKindStmt
		const getStmt = table === "names" ? this.getNameIdStmt : this.getKindIdStmt

		insertStmt.bind([text])
		insertStmt.step()
		insertStmt.reset()

		getStmt.bind([text])
		if (!getStmt.step()) {
			getStmt.reset()
			throw new Error(`Failed to get ID from ${table} for text: ${text}`)
		}
		const id = (getStmt.getAsObject() as any).id
		getStmt.reset()

		return id
	}

	public async updateFileSymbols(
		relPath: string,
		mtime: number,
		size: number,
		symbols: Array<{
			n: string
			t: "d" | "r" | "a" | "i" // i = import/include
			k?: string
			r: [number, number, number, number] | Uint32Array
		}>,
	): Promise<void> {
		this.isDirty = true
		this.db.run("BEGIN TRANSACTION")
		try {
			// Ensure file exists and get its ID
			this.db.run("INSERT OR REPLACE INTO files (path, mtime, size) VALUES (?, ?, ?)", [relPath, mtime, size])
			const fileId = this.getFileId(relPath)

			// Clear old symbols and dependencies for this file
			this.db.run("DELETE FROM symbols WHERE file_id = ?", [fileId])
			this.db.run("DELETE FROM dependencies WHERE file_id = ?", [fileId])

			for (const sym of symbols) {
				const nameId = this.getOrCreateId("names", sym.n)
				const kindId = sym.k ? this.getOrCreateId("kinds", sym.k) : null

				const range = Array.isArray(sym.r) ? sym.r : [sym.r[0], sym.r[1], sym.r[2], sym.r[3]]

				this.insertSymbolStmt.run([
					fileId,
					nameId,
					sym.t, // "d", "r", "a", or "i"
					kindId,
					range[0],
					range[1],
					range[2],
					range[3],
				])
			}
			this.db.run("COMMIT")
		} catch (error) {
			this.db.run("ROLLBACK")
			throw error
		}
	}

	private getFileId(relPath: string): number {
		const idStmt = this.db.prepare("SELECT id FROM files WHERE path = ?")
		idStmt.bind([relPath])
		if (!idStmt.step()) {
			idStmt.free()
			throw new Error(`Failed to get ID for file: ${relPath}`)
		}
		const fileId = (idStmt.getAsObject() as any).id
		idStmt.free()
		return fileId
	}

	public addDependency(relPath: string, dependencyRelPath: string): void {
		this.isDirty = true
		try {
			const fileId = this.getFileId(relPath)
			// Dependency might not be indexed yet, but it must exist in the files table for the FK to work.
			// If it's not indexed, we insert a placeholder record.
			this.db.run("INSERT OR IGNORE INTO files (path, mtime, size) VALUES (?, 0, 0)", [dependencyRelPath])
			const depId = this.getFileId(dependencyRelPath)
			this.db.run("INSERT OR IGNORE INTO dependencies (file_id, dependency_id) VALUES (?, ?)", [fileId, depId])
		} catch (error) {
			Logger.warn(`[SymbolIndexDatabase] Failed to add dependency ${relPath} -> ${dependencyRelPath}:`, error)
		}
	}

	public getDependencies(relPath: string): string[] {
		try {
			const fileId = this.getFileId(relPath)
			const stmt = this.db.prepare(
				"SELECT f.path FROM dependencies d JOIN files f ON d.dependency_id = f.id WHERE d.file_id = ?",
			)
			stmt.bind([fileId])
			const results: string[] = []
			while (stmt.step()) {
				results.push((stmt.getAsObject() as any).path)
			}
			stmt.free()
			return results
		} catch {
			return []
		}
	}

	public updateFilesSymbolsBatch(
		updates: Array<{
			relPath: string
			mtime: number
			size: number
			symbols: Array<{
				n: string
				t: "d" | "r" | "a" | "i"
				k?: string
				r: [number, number, number, number] | Uint32Array
			}>
		}>,
	): void {
		this.isDirty = true
		this.db.run("BEGIN TRANSACTION")
		try {
			for (const update of updates) {
				// Ensure file exists and get its ID
				this.db.run("INSERT OR REPLACE INTO files (path, mtime, size) VALUES (?, ?, ?)", [
					update.relPath,
					update.mtime,
					update.size,
				])
				const idStmt = this.db.prepare("SELECT id FROM files WHERE path = ?")
				idStmt.bind([update.relPath])
				if (!idStmt.step()) {
					idStmt.free()
					continue
				}
				const fileId = (idStmt.getAsObject() as any).id
				idStmt.free()

				// Clear old symbols and dependencies for this file
				this.db.run("DELETE FROM symbols WHERE file_id = ?", [fileId])
				this.db.run("DELETE FROM dependencies WHERE file_id = ?", [fileId])

				for (const sym of update.symbols) {
					const nameId = this.getOrCreateId("names", sym.n)
					const kindId = sym.k ? this.getOrCreateId("kinds", sym.k) : null

					const range = Array.isArray(sym.r) ? sym.r : [sym.r[0], sym.r[1], sym.r[2], sym.r[3]]

					this.insertSymbolStmt.run([
						fileId,
						nameId,
						sym.t, // "d", "r", "a", or "i"
						kindId,
						range[0],
						range[1],
						range[2],
						range[3],
					])
				}
			}
			this.db.run("COMMIT")
		} catch (error) {
			this.db.run("ROLLBACK")
			throw error
		}
	}

	public removeFile(relPath: string): void {
		this.isDirty = true
		this.db.run("DELETE FROM files WHERE path = ?", [relPath])
	}

	public getSymbolsByName(
		name: string,
		type?: "definition" | "reference" | "declaration",
		limit?: number,
	): SymbolLocation[] {
		let query =
			"SELECT f.path as file_path, n.text as name, s.type, k.text as kind, s.start_line, s.start_column, s.end_line, s.end_column " +
			"FROM symbols s " +
			"JOIN files f ON s.file_id = f.id " +
			"JOIN names n ON s.name_id = n.id " +
			"LEFT JOIN kinds k ON s.kind_id = k.id " +
			"WHERE n.text = ?"
		const params: any[] = [name]

		if (type) {
			query += " AND s.type = ?"
			// Map public type names to internal compact format
			const typeMap: Record<string, string> = {
				definition: "d",
				reference: "r",
				declaration: "a",
			}
			params.push(typeMap[type])
		}

		if (limit !== undefined) {
			query += " LIMIT ?"
			params.push(limit)
		}

		const stmt = this.db.prepare(query)
		stmt.bind(params)
		const results: SymbolLocation[] = []
		const reverseTypeMap: Record<string, "definition" | "reference" | "declaration"> = {
			d: "definition",
			r: "reference",
			a: "declaration",
		}

		while (stmt.step()) {
			const row = stmt.getAsObject() as any
			results.push({
				path: row.file_path,
				startLine: row.start_line,
				startColumn: row.start_column,
				endLine: row.end_line,
				endColumn: row.end_column,
				type: (reverseTypeMap[row.type] || "reference") as any,
				kind: row.kind || undefined,
			})
		}
		stmt.free()
		return results
	}

	public close(): void {
		void this.save()
		this.insertNameStmt?.free()
		this.getNameIdStmt?.free()
		this.insertKindStmt?.free()
		this.getKindIdStmt?.free()
		this.insertSymbolStmt?.free()
		this.db.close()
	}
}
