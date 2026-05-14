use ratatui::text::Line;
use std::collections::HashSet;

/// Cached per-block rendered lines and visual line counts.
/// Invalidated when width, generation, expand state, or wrap state changes.
pub(crate) struct LineCache {
    pub(crate) width: u16,
    pub(crate) generation: u64,
    pub(crate) expanded: HashSet<usize>,
    pub(crate) wrapped: HashSet<usize>,
    /// Per-block visual line count after wrapping.
    pub(crate) per_block: Vec<usize>,
    /// Total visual lines (blocks only, excluding streaming/pending).
    pub(crate) blocks_total: usize,
    /// Cached rendered `Line` objects for each block (without selection marker/highlight).
    pub(crate) cached_block_lines: Vec<Vec<Line<'static>>>,
}

impl LineCache {
    pub(crate) fn is_valid(
        &self, width: u16, generation: u64,
        expanded: &HashSet<usize>, wrapped: &HashSet<usize>,
    ) -> bool {
        self.width == width
            && self.generation == generation
            && self.expanded == *expanded
            && self.wrapped == *wrapped
    }
}
