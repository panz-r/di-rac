pub mod secrets;

use std::collections::HashMap;

/// Stable content hash using blake3. Returns first 16 hex chars.
/// Suitable for artifact content hashes, file read hashes, and any
/// hash that must be consistent across process restarts.
pub fn stable_hash(data: &[u8]) -> String {
    let hash = blake3::hash(data);
    format!("{:.16}", hash.to_hex())
}

/// Fast diagnostic hash using blake3. Returns first 16 hex chars.
/// Suitable for frame region fingerprints, cache keys, and other
/// in-process diagnostics where cross-process stability is valued
/// but the hash never leaves the process.
pub fn fast_hash(data: &[u8]) -> String {
    let hash = blake3::hash(data);
    format!("{:.16}", hash.to_hex())
}

/// Base-32 alphabet matching the TS anchor system: digits + lowercase a-v.
/// Using 3 chars gives 32^3 = 32,768 distinct values (8x better than hex's 4,096).
const ANCHOR_ALPHABET: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";

/// Encode a blake3 hash as a base-32 string using the anchor alphabet.
/// Takes the first 4 bytes of the blake3 output as a u32, then divides
/// repeatedly by 32 to extract base-32 digits.
pub fn anchor_hash(data: &[u8]) -> String {
    let hash = blake3::hash(data);
    // Take first 4 bytes as a u32 for base-32 encoding
    let mut remaining = u32::from_be_bytes(hash.as_bytes()[..4].try_into().unwrap_or([0; 4]));
    let mut result = [0u8; 3];
    for i in (0..3).rev() {
        result[i] = ANCHOR_ALPHABET[(remaining % 32) as usize];
        remaining /= 32;
    }
    String::from_utf8(result.to_vec()).unwrap_or_default()
}

/// Deterministic index mapping line numbers to content hashes with
/// collision resolution. When two lines produce the same 3-char hash,
/// they get suffixed `_0`, `_1`, etc.
///
/// Port of `src/shared/utils/file-anchor-index.ts`.
pub struct FileAnchorIndex {
    hash_by_line: Vec<String>,
    line_idx_by_hash: HashMap<String, usize>,
    lines: Vec<String>,
}

impl FileAnchorIndex {
    /// Build an anchor index from lines (without trailing newlines).
    /// Two-pass: compute raw hashes, then resolve collisions.
    pub fn new(lines: &[&str]) -> Self {
        let mut collision_map: HashMap<String, Vec<usize>> = HashMap::new();
        let mut raw_hashes: Vec<String> = Vec::with_capacity(lines.len());

        // First pass: compute raw hashes and detect collisions
        for (i, line) in lines.iter().enumerate() {
            let raw = anchor_hash(line.as_bytes());
            raw_hashes.push(raw.clone());
            collision_map.entry(raw).or_default().push(i);
        }

        // Second pass: assign final anchors with collision suffixes
        let mut hash_by_line = Vec::with_capacity(lines.len());
        let mut line_idx_by_hash = HashMap::with_capacity(lines.len());
        for (i, raw) in raw_hashes.iter().enumerate() {
            let indices = &collision_map[raw];
            let final_hash = if indices.len() == 1 {
                raw.clone()
            } else {
                let collision_index = indices.iter().position(|&x| x == i).unwrap_or(0);
                format!("{}_{}", raw, collision_index)
            };
            line_idx_by_hash.insert(final_hash.clone(), i);
            hash_by_line.push(final_hash);
        }

        let lines = lines.iter().map(|s| s.to_string()).collect();
        Self { hash_by_line, line_idx_by_hash, lines }
    }

    /// Returns the anchor hash for the given line index (0-based).
    pub fn get_hash(&self, line_idx: usize) -> &str {
        self.hash_by_line.get(line_idx).map(|s| s.as_str()).unwrap_or("")
    }

    /// Returns all anchor hashes in order.
    pub fn get_all_hashes(&self) -> &[String] {
        &self.hash_by_line
    }

    /// Number of lines in the index.
    pub fn line_count(&self) -> usize {
        self.hash_by_line.len()
    }

    /// Reverse lookup: hash -> 0-based line index. Returns None if not found.
    pub fn get_line_idx(&self, hash: &str) -> Option<usize> {
        self.line_idx_by_hash.get(hash).copied()
    }

    /// Get the line content at the given 0-based index. Returns "" if out of bounds.
    pub fn get_line(&self, line_idx: usize) -> &str {
        self.lines.get(line_idx).map(|s| s.as_str()).unwrap_or("")
    }

    /// Get all lines as a slice.
    pub fn get_lines(&self) -> &[String] {
        &self.lines
    }

    /// After a line is modified in-place, recompute its hash and update both maps.
    /// Handles collision resolution: if new raw hash collides with another line's
    /// hash, tries suffixes _0, _1, ... until a free slot is found.
    pub fn update_line(&mut self, line_idx: usize, new_content: &str) {
        if line_idx >= self.lines.len() {
            return;
        }

        // Remove old hash mapping
        let old_hash = self.hash_by_line[line_idx].clone();
        self.line_idx_by_hash.remove(&old_hash);

        // Update line content
        self.lines[line_idx] = new_content.to_string();

        // Compute new raw hash
        let raw = anchor_hash(new_content.as_bytes());

        // Resolve collisions: find a unique hash
        let final_hash = if !self.line_idx_by_hash.contains_key(&raw) {
            raw
        } else {
            let mut suffix = 0;
            loop {
                let candidate = format!("{}_{}", raw, suffix);
                if !self.line_idx_by_hash.contains_key(&candidate) {
                    break candidate;
                }
                suffix += 1;
            }
        };

        self.line_idx_by_hash.insert(final_hash.clone(), line_idx);
        self.hash_by_line[line_idx] = final_hash;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_lines_get_raw_hashes() {
        let lines = vec!["fn main() {", "    println!(\"hello\");", "}"];
        let idx = FileAnchorIndex::new(&lines);
        assert_eq!(idx.line_count(), 3);
        // All hashes should be 3 chars, no collisions expected
        for i in 0..3 {
            let h = idx.get_hash(i);
            assert_eq!(h.len(), 3);
            assert!(!h.contains('_'));
        }
    }

    #[test]
    fn collision_lines_get_suffixed_hashes() {
        // Many identical lines should trigger collisions
        let lines: Vec<&str> = vec!["same", "same", "same", "same"];
        let idx = FileAnchorIndex::new(&lines);
        assert_eq!(idx.line_count(), 4);
        // First gets raw hash, rest get _1, _2, _3
        let h0 = idx.get_hash(0);
        assert!(h0.contains('_'));
        assert!(h0.ends_with("_0"));
        assert!(idx.get_hash(1).ends_with("_1"));
        assert!(idx.get_hash(2).ends_with("_2"));
        assert!(idx.get_hash(3).ends_with("_3"));
    }

    #[test]
    fn mixed_unique_and_colliding() {
        let lines: Vec<&str> = vec!["unique_a", "same", "unique_b", "same"];
        let idx = FileAnchorIndex::new(&lines);
        // Lines 0 and 2 are unique -> raw 3-char hash
        assert!(!idx.get_hash(0).contains('_'));
        assert!(!idx.get_hash(2).contains('_'));
        // Lines 1 and 3 collide -> suffixed
        assert!(idx.get_hash(1).ends_with("_0"));
        assert!(idx.get_hash(3).ends_with("_1"));
    }

    #[test]
    fn empty_input() {
        let lines: Vec<&str> = vec![];
        let idx = FileAnchorIndex::new(&lines);
        assert_eq!(idx.line_count(), 0);
    }

    #[test]
    fn reverse_lookup() {
        let lines = vec!["fn main() {", "    println!(\"hello\");", "}"];
        let idx = FileAnchorIndex::new(&lines);
        // Forward and reverse must agree
        for i in 0..3 {
            let h = idx.get_hash(i);
            assert_eq!(idx.get_line_idx(h), Some(i));
        }
        // Unknown hash returns None
        assert_eq!(idx.get_line_idx("zzz"), None);
    }

    #[test]
    fn get_line_content() {
        let lines = vec!["alpha", "beta", "gamma"];
        let idx = FileAnchorIndex::new(&lines);
        assert_eq!(idx.get_line(0), "alpha");
        assert_eq!(idx.get_line(2), "gamma");
        assert_eq!(idx.get_line(99), "");
    }

    #[test]
    fn update_line_rehashes() {
        let lines = vec!["aaa", "bbb", "ccc"];
        let mut idx = FileAnchorIndex::new(&lines);
        let old_hash = idx.get_hash(1).to_string();
        idx.update_line(1, "changed");
        let new_hash = idx.get_hash(1).to_string();
        assert_ne!(old_hash, new_hash);
        assert_eq!(idx.get_line(1), "changed");
        // Reverse lookup updated
        assert_eq!(idx.get_line_idx(&new_hash), Some(1));
        assert_eq!(idx.get_line_idx(&old_hash), None);
    }

    #[test]
    fn update_line_handles_collision() {
        // Two identical lines; update one to match a different line that also collides
        let lines = vec!["same", "diff"];
        let mut idx = FileAnchorIndex::new(&lines);
        // Update line 1 to "same" — raw hash matches line 0's raw hash
        // but line 0 has suffixed hash, so raw hash is free
        idx.update_line(1, "same");
        // Line 1 should have a valid hash that reverse-lookups correctly
        let h1 = idx.get_hash(1);
        assert_eq!(idx.get_line_idx(h1), Some(1));
        // Both lines have same content, both should be resolvable
        let h0 = idx.get_hash(0);
        assert_eq!(idx.get_line_idx(h0), Some(0));
        assert_ne!(h0, h1);
    }
}
