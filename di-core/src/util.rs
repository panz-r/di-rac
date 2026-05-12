pub mod secrets;

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
