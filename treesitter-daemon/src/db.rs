use rusqlite::{Connection, Result as SqliteResult};
use std::path::Path;
use std::sync::Mutex;

/// Persistent SQLite index database for symbols, references, and imports.
pub struct IndexDatabase {
    db: Mutex<Connection>,
}

impl IndexDatabase {
    /// Open (or create) the index database at the given path.
    pub fn open(db_path: &Path) -> SqliteResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(db_path)?;
        let db = Self { db: Mutex::new(conn) };
        db.create_schema()?;
        Ok(db)
    }

    /// Run CREATE TABLE IF NOT EXISTS for all tables and indexes.
    fn create_schema(&self) -> SqliteResult<()> {
        let conn = self.db.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT UNIQUE NOT NULL,
                mtime REAL NOT NULL,
                content_hash TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS symbols (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                type TEXT NOT NULL,
                kind TEXT,
                start_line INTEGER NOT NULL,
                start_col INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                end_col INTEGER NOT NULL,
                handle TEXT
            );

            CREATE TABLE IF NOT EXISTS imports (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                module TEXT NOT NULL,
                names TEXT,
                line INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_id);
            CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_handle ON symbols(handle);
            CREATE INDEX IF NOT EXISTS idx_imports_file ON imports(file_id);
            "#,
        )
    }

    /// Index (upsert) a file's symbols and imports into the database.
    /// Returns the number of symbols indexed.
    pub fn index_file(
        &self,
        file_path: &str,
        mtime: f64,
        content_hash: &str,
        symbols: &[super::indexer::IndexedSymbol],
        imports: &[super::extractor::Import],
    ) -> SqliteResult<usize> {
        let mut conn = self.db.lock().unwrap();
        let tx = conn.transaction()?;

        // Upsert file record
        let file_id: i64 = tx
            .query_row(
                "SELECT id FROM files WHERE path = ?1",
                [file_path],
                |row| row.get(0),
            )
            .or_else(|_| {
                tx.execute("INSERT INTO files (path, mtime, content_hash) VALUES (?1, ?2, ?3)", rusqlite::params![file_path, mtime, content_hash])?;
                Ok::<i64, rusqlite::Error>(tx.last_insert_rowid())
            })?;

        // Delete existing symbols and imports for this file
        tx.execute("DELETE FROM symbols WHERE file_id = ?1", [file_id])?;
        tx.execute("DELETE FROM imports WHERE file_id = ?1", [file_id])?;

        // Insert symbols
        let symbol_count = symbols.len();
        if !symbols.is_empty() {
            let mut stmt = tx.prepare(
                "INSERT INTO symbols (file_id, name, type, kind, start_line, start_col, end_line, end_col)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for sym in symbols {
                stmt.execute(rusqlite::params![
                    file_id,
                    &sym.n,
                    &sym.t,
                    &sym.k,
                    sym.start_line as i64,
                    sym.start_col as i64,
                    sym.end_line as i64,
                    sym.end_col as i64,
                ])?;
            }
        }

        // Insert imports
        if !imports.is_empty() {
            let mut imp_stmt = tx.prepare(
                "INSERT INTO imports (file_id, module, names, line) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for imp in imports {
                let names_json = if imp.names.is_empty() {
                    String::new()
                } else {
                    serde_json::to_string(&imp.names).unwrap_or_default()
                };
                imp_stmt.execute(rusqlite::params![
                    file_id,
                    &imp.module,
                    &names_json,
                    imp.line as i64,
                ])?;
            }
        }

        tx.commit()?;
        Ok(symbol_count)
    }

    /// Remove all entries (symbols, imports) for a file.
    pub fn invalidate_file(&self, file_path: &str) -> SqliteResult<()> {
        let mut conn = self.db.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM symbols WHERE file_id IN (SELECT id FROM files WHERE path = ?1)",
            [file_path],
        )?;
        tx.execute(
            "DELETE FROM imports WHERE file_id IN (SELECT id FROM files WHERE path = ?1)",
            [file_path],
        )?;
        tx.execute("DELETE FROM files WHERE path = ?1", [file_path])?;
        tx.commit()?;
        Ok(())
    }

    /// Search symbols by name (LIKE %query%).
    pub fn search_symbols(
        &self,
        query: &str,
        kind_filter: Option<&str>,
        max_results: usize,
    ) -> SqliteResult<Vec<SearchResult>> {
        let conn = self.db.lock().unwrap();
        let pattern = format!("%{}%", query);

        let mut results = Vec::new();

        if let Some(kf) = kind_filter {
            let mut stmt = conn.prepare(
                "SELECT s.name, s.type, s.kind, s.start_line, s.start_col, s.end_line, s.end_col, f.path
                 FROM symbols s JOIN files f ON s.file_id = f.id
                 WHERE s.name LIKE ?1 AND s.kind = ?2
                 LIMIT ?3",
            )?;
            let mut rows = stmt.query(rusqlite::params![&pattern, kf, max_results as i64])?;
            while let Some(row) = rows.next()? {
                results.push(SearchResult::from_row(row));
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT s.name, s.type, s.kind, s.start_line, s.start_col, s.end_line, s.end_col, f.path
                 FROM symbols s JOIN files f ON s.file_id = f.id
                 WHERE s.name LIKE ?1
                 LIMIT ?2",
            )?;
            let mut rows = stmt.query(rusqlite::params![&pattern, max_results as i64])?;
            while let Some(row) = rows.next()? {
                results.push(SearchResult::from_row(row));
            }
        }

        Ok(results)
    }

    /// Get index statistics.
    pub fn index_status(&self) -> SqliteResult<IndexStatus> {
        let conn = self.db.lock().unwrap();
        let file_count: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        let symbol_count: i64 = conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))?;
        let import_count: i64 = conn.query_row("SELECT COUNT(*) FROM imports", [], |r| r.get(0))?;
        Ok(IndexStatus {
            file_count: file_count as usize,
            symbol_count: symbol_count as usize,
            import_count: import_count as usize,
        })
    }

    /// Delete all entries from the index.
    pub fn clear_index(&self) -> SqliteResult<()> {
        let conn = self.db.lock().unwrap();
        conn.execute_batch("DELETE FROM symbols; DELETE FROM imports; DELETE FROM files;")
    }
}

pub struct SearchResult {
    pub name: String,
    pub type_: String, // "d", "r", "a", "i"
    pub kind: String,
    pub file_path: String,
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl SearchResult {
    pub fn from_row(row: &rusqlite::Row) -> Self {
        Self {
            name: row.get(0).unwrap_or_default(),
            type_: row.get::<_, Option<String>>(1).unwrap_or(None).unwrap_or_default(),
            kind: row.get::<_, Option<String>>(2).unwrap_or(None).unwrap_or_default(),
            file_path: row.get(7).unwrap_or_default(),
            start_line: row.get::<_, i64>(3).unwrap_or(0) as usize,
            start_col: row.get::<_, i64>(4).unwrap_or(0) as usize,
            end_line: row.get::<_, i64>(5).unwrap_or(0) as usize,
            end_col: row.get::<_, i64>(6).unwrap_or(0) as usize,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IndexStatus {
    pub file_count: usize,
    pub symbol_count: usize,
    pub import_count: usize,
}

#[cfg(test)]
mod tests {
    use super::{IndexDatabase, IndexStatus, SearchResult};
    use crate::indexer::IndexedSymbol;
    use crate::extractor::Import;
    use std::fs;

    fn test_db_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("dirac-db-test").join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("data.db")
    }

    fn make_indexed_symbol(name: &str, t: &str, k: Option<&str>, line: usize) -> IndexedSymbol {
        IndexedSymbol {
            n: name.to_string(),
            t: t.to_string(),
            k: k.map(String::from),
            start_line: line,
            start_col: 1,
            end_line: line + 10,
            end_col: 10,
        }
    }

    fn make_import(module: &str, names: Vec<&str>, line: usize) -> Import {
        Import {
            module: module.to_string(),
            names: names.into_iter().map(String::from).collect(),
            line,
        }
    }

    #[test]
    fn test_index_file_and_search() {
        let db_path = test_db_path("test_index_file_and_search");
        let db = IndexDatabase::open(&db_path).unwrap();

        let symbols = vec![
            make_indexed_symbol("login", "d", Some("function"), 10),
            make_indexed_symbol("AuthService", "d", Some("class"), 20),
            make_indexed_symbol("user", "r", None, 30),
        ];
        let imports = vec![
            make_import("os", vec!["path"], 5),
            make_import("sys", vec![], 6),
        ];

        let count = db.index_file("src/main.rs", 1234567890.0, "abc123", &symbols, &imports).unwrap();
        assert_eq!(count, 3);

        // Search for "login"
        let results = db.search_symbols("login", None, 100).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "login");
        assert_eq!(results[0].kind, "function");
        assert_eq!(results[0].file_path, "src/main.rs");

        // Search for "Auth"
        let results = db.search_symbols("Auth", None, 100).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "AuthService");
        assert_eq!(results[0].kind, "class");

        // Search with kind filter
        let results = db.search_symbols("login", Some("class"), 100).unwrap();
        assert_eq!(results.len(), 0);

        let results = db.search_symbols("login", Some("function"), 100).unwrap();
        assert_eq!(results.len(), 1);

        // Search for non-existent
        let results = db.search_symbols("nonexistent", None, 100).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_invalidate_file() {
        let db_path = test_db_path("test_invalidate_file");
        let db = IndexDatabase::open(&db_path).unwrap();

        let symbols = vec![make_indexed_symbol("foo", "d", Some("function"), 10)];
        db.index_file("src/foo.py", 0.0, "hash1", &symbols, &[]).unwrap();

        let status = db.index_status().unwrap();
        assert_eq!(status.file_count, 1);
        assert_eq!(status.symbol_count, 1);

        db.invalidate_file("src/foo.py").unwrap();

        let status = db.index_status().unwrap();
        assert_eq!(status.file_count, 0);
        assert_eq!(status.symbol_count, 0);

        let results = db.search_symbols("foo", None, 100).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_index_status() {
        let db_path = test_db_path("test_index_status");
        let db = IndexDatabase::open(&db_path).unwrap();

        let status = db.index_status().unwrap();
        assert_eq!(status.file_count, 0);
        assert_eq!(status.symbol_count, 0);
        assert_eq!(status.import_count, 0);

        let symbols = vec![
            make_indexed_symbol("a", "d", Some("function"), 1),
            make_indexed_symbol("b", "d", Some("class"), 2),
        ];
        let imports = vec![
            make_import("x", vec!["y", "z"], 1),
            make_import("w", vec![], 2),
        ];
        db.index_file("f1.py", 0.0, "h1", &symbols, &imports).unwrap();

        let symbols2 = vec![make_indexed_symbol("c", "d", Some("method"), 3)];
        db.index_file("f2.py", 0.0, "h2", &symbols2, &[]).unwrap();

        let status = db.index_status().unwrap();
        assert_eq!(status.file_count, 2);
        assert_eq!(status.symbol_count, 3);
        assert_eq!(status.import_count, 2);
    }

    #[test]
    fn test_clear_index() {
        let db_path = test_db_path("test_clear_index");
        let db = IndexDatabase::open(&db_path).unwrap();

        let symbols = vec![make_indexed_symbol("x", "d", None, 1)];
        db.index_file("a.py", 0.0, "h", &symbols, &[]).unwrap();
        db.index_file("b.py", 0.0, "h", &symbols, &[]).unwrap();

        let status = db.index_status().unwrap();
        assert_eq!(status.file_count, 2);

        db.clear_index().unwrap();

        let status = db.index_status().unwrap();
        assert_eq!(status.file_count, 0);
        assert_eq!(status.symbol_count, 0);
        assert_eq!(status.import_count, 0);
    }

    #[test]
    fn test_reindex_same_file() {
        let db_path = test_db_path("test_reindex_same_file");
        let db = IndexDatabase::open(&db_path).unwrap();

        let symbols1 = vec![make_indexed_symbol("func", "d", Some("function"), 10)];
        db.index_file("src/lib.py", 1000.0, "hash_v1", &symbols1, &[]).unwrap();

        let status = db.index_status().unwrap();
        assert_eq!(status.symbol_count, 1);

        // Re-index with different symbols
        let symbols2 = vec![
            make_indexed_symbol("func", "d", Some("function"), 10),
            make_indexed_symbol("func2", "d", Some("function"), 20),
        ];
        db.index_file("src/lib.py", 2000.0, "hash_v2", &symbols2, &[]).unwrap();

        let status = db.index_status().unwrap();
        assert_eq!(status.file_count, 1); // still 1 file
        assert_eq!(status.symbol_count, 2); // now 2 symbols

        // Search shows both
        let results = db.search_symbols("func", None, 100).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_multiple_files_same_symbol_name() {
        let db_path = test_db_path("test_multiple_files_same_symbol_name");
        let db = IndexDatabase::open(&db_path).unwrap();

        let sym = vec![make_indexed_symbol("Helper", "d", Some("class"), 5)];
        db.index_file("utils.py", 0.0, "h1", &sym, &[]).unwrap();
        db.index_file("models.py", 0.0, "h2", &sym, &[]).unwrap();
        db.index_file("auth.py", 0.0, "h3", &sym, &[]).unwrap();

        let results = db.search_symbols("Helper", None, 100).unwrap();
        assert_eq!(results.len(), 3);
        let paths: Vec<_> = results.iter().map(|r| r.file_path.as_str()).collect();
        assert!(paths.contains(&"utils.py"));
        assert!(paths.contains(&"models.py"));
        assert!(paths.contains(&"auth.py"));
    }
}