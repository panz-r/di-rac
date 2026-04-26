import * as fs from "fs"
import * as path from "path"
import type { Database } from "sql.js"
import initSqlJs from "sql.js"
import { Logger } from "../../shared/services/Logger"
import { SymbolLocation } from "./SymbolIndexService"

export interface FileMetadata {
	mtime: number
	size: number
}

export class SymbolIndexDatabase {
	private db: Database
	private dbPath: string
	private isDirty = false

	private constructor(db: Database, dbPath: string) {
		this.db = db
		this.dbPath = dbPath
	}

	public static async create(dbPath: string): Promise<SymbolIndexDatabase> {
		Logger.info(`[SymbolIndexDatabase] Initializing database at ${dbPath}`)
		const dbDir = path.dirname(dbPath)
		if (!fs.existsSync(dbDir)) {
			Logger.info(`[SymbolIndexDatabase] Creating database directory: ${dbDir}`)
			fs.mkdirSync(dbDir, { recursive: true })
		}

		const SQL = await initSqlJs({
			locateFile: (file) => path.join(__dirname, file),
		})
		let db: Database

		if (fs.existsSync(dbPath)) {
			Logger.info(`[SymbolIndexDatabase] Loading existing database from ${dbPath}`)
			const fileBuffer = fs.readFileSync(dbPath)
			db = new SQL.Database(fileBuffer)
		} else {
			Logger.info(`[SymbolIndexDatabase] Creating new database`)
			db = new SQL.Database()
		}

		const instance = new SymbolIndexDatabase(db, dbPath)
		instance.initialize()
		return instance
	}

	private initialize(): void {
		Logger.info("[SymbolIndexDatabase] Running schema initialization")
		this.db.run("PRAGMA foreign_keys = ON")

		// Create files table with ID
		this.db.run(`
			CREATE TABLE IF NOT EXISTS files (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				path TEXT UNIQUE NOT NULL,
				mtime INTEGER NOT NULL,
				size INTEGER NOT NULL
			);
		`)

		// Create symbols table referencing file_id
		this.db.run(`
			CREATE TABLE IF NOT EXISTS symbols (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				file_id INTEGER NOT NULL,
				name TEXT NOT NULL,
				type TEXT NOT NULL,
				kind TEXT,
				start_line INTEGER NOT NULL,
				start_column INTEGER NOT NULL,
				end_line INTEGER NOT NULL,
				end_column INTEGER NOT NULL,
				FOREIGN KEY (file_id) REFERENCES files(id) ON DELETE CASCADE
			);
		`)

		this.db.run("CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name)")
		this.db.run("CREATE INDEX IF NOT EXISTS idx_symbols_name_type ON symbols(name, type)")
		this.db.run("CREATE INDEX IF NOT EXISTS idx_symbols_file_id ON symbols(file_id)")

		// Perform migration if needed (detect old schema)
		try {
			const checkStmt = this.db.prepare("PRAGMA table_info(symbols)")
			let hasFilePath = false
			while (checkStmt.step()) {
				const column = checkStmt.getAsObject() as any
				if (column.name === "file_path") {
					hasFilePath = true
					break
				}
			}
			checkStmt.free()

			if (hasFilePath) {
				Logger.info("[SymbolIndexDatabase] Migrating database to normalized schema")
				// The easiest "migration" for a cache is to clear it and let it re-index
				// This avoids complex SQL migration logic while ensuring a clean state
				this.db.run("DROP TABLE symbols")
				this.db.run("DROP TABLE files")
				this.initialize() // Re-run to create new tables
				return
			}
		} catch (error) {
			Logger.warn("[SymbolIndexDatabase] Migration check failed, ignoring:", error)
		}

		Logger.info("[SymbolIndexDatabase] Schema initialization complete")
	}

	public save(): void {
		if (!this.isDirty) {
			return
		}
		Logger.info(`[SymbolIndexDatabase] Saving database to ${this.dbPath}`)
		const data = this.db.export()
		const buffer = Buffer.from(data)
		fs.writeFileSync(this.dbPath, buffer)
		this.isDirty = false
		Logger.info(`[SymbolIndexDatabase] Database saved successfully`)
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

	public updateFileSymbols(
		relPath: string,
		mtime: number,
		size: number,
		symbols: Array<{
			n: string
			t: "d" | "r" | "a"
			k?: string
			r: [number, number, number, number]
		}>,
	): void {
		this.isDirty = true
		this.db.run("BEGIN TRANSACTION")
		try {
			// Ensure file exists and get its ID
			this.db.run("INSERT OR REPLACE INTO files (path, mtime, size) VALUES (?, ?, ?)", [relPath, mtime, size])
			const idStmt = this.db.prepare("SELECT id FROM files WHERE path = ?")
			idStmt.bind([relPath])
			if (!idStmt.step()) {
				idStmt.free()
				throw new Error(`Failed to get ID for file: ${relPath}`)
			}
			const fileId = (idStmt.getAsObject() as any).id
			idStmt.free()

			// Clear old symbols for this file
			this.db.run("DELETE FROM symbols WHERE file_id = ?", [fileId])

			const insertSymbol = this.db.prepare(`
				INSERT INTO symbols (file_id, name, type, kind, start_line, start_column, end_line, end_column)
				VALUES (?, ?, ?, ?, ?, ?, ?, ?)
			`)

			for (const sym of symbols) {
				insertSymbol.run([
					fileId,
					sym.n,
					sym.t, // "d", "r", or "a"
					sym.k || null,
					sym.r[0],
					sym.r[1],
					sym.r[2],
					sym.r[3],
				])
			}
			insertSymbol.free()
			this.db.run("COMMIT")
		} catch (error) {
			this.db.run("ROLLBACK")
			throw error
		}
	}

	public updateFilesSymbolsBatch(
		updates: Array<{
			relPath: string
			mtime: number
			size: number
			symbols: Array<{
				n: string
				t: "d" | "r" | "a"
				k?: string
				r: [number, number, number, number]
			}>
		}>,
	): void {
		this.isDirty = true
		this.db.run("BEGIN TRANSACTION")
		try {
			const insertSymbol = this.db.prepare(`
				INSERT INTO symbols (file_id, name, type, kind, start_line, start_column, end_line, end_column)
				VALUES (?, ?, ?, ?, ?, ?, ?, ?)
			`)

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

				// Clear old symbols for this file
				this.db.run("DELETE FROM symbols WHERE file_id = ?", [fileId])

				for (const sym of update.symbols) {
					insertSymbol.run([
						fileId,
						sym.n,
						sym.t, // "d", "r", or "a"
						sym.k || null,
						sym.r[0],
						sym.r[1],
						sym.r[2],
						sym.r[3],
					])
				}
			}
			insertSymbol.free()
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
			"SELECT f.path as file_path, s.name, s.type, s.kind, s.start_line, s.start_column, s.end_line, s.end_column " +
			"FROM symbols s JOIN files f ON s.file_id = f.id " +
			"WHERE s.name = ?"
		const params: any[] = [name]

		if (type) {
			query += " AND s.type = ?"
			// Map public type names to internal compact format
			const typeMap = {
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
				type: reverseTypeMap[row.type] || "reference",
				kind: row.kind || undefined,
			})
		}
		stmt.free()
		return results
	}

	public close(): void {
		this.save()
		this.db.close()
	}
}
