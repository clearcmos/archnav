//! The rows picked in the results list with Ctrl and Shift, as a set of row
//! numbers that stays small for ranges (a Shift+click over a million rows).

use roaring::RoaringBitmap;

#[derive(Debug, Default)]
pub struct RowSelection {
    rows: RoaringBitmap,
}

impl RowSelection {
    pub fn len(&self) -> u64 {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn clear(&mut self) {
        self.rows.clear();
    }

    pub fn contains(&self, row: i32) -> bool {
        u32::try_from(row).is_ok_and(|row| self.rows.contains(row))
    }

    /// Add `row` if it is not selected, otherwise remove it. Rows outside
    /// `0..row_count` are ignored.
    pub fn toggle(&mut self, row: i32, row_count: u32) {
        if let Ok(row) = u32::try_from(row) {
            if row < row_count && !self.rows.remove(row) {
                self.rows.insert(row);
            }
        }
    }

    /// Select the rows from `from` to `to`, either order, clamped to
    /// `0..row_count`; with `keep`, on top of what is already selected.
    pub fn select_range(&mut self, from: i32, to: i32, keep: bool, row_count: u32) {
        if !keep {
            self.rows.clear();
        }
        if row_count == 0 {
            return;
        }
        let clamp = |row: i32| (row.max(0) as u32).min(row_count - 1);
        self.rows
            .insert_range(clamp(from.min(to))..=clamp(from.max(to)));
    }

    /// Selected rows in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.rows.iter().map(|row| row as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(s: &RowSelection) -> Vec<usize> {
        s.iter().collect()
    }

    #[test]
    fn toggling_adds_and_removes_within_bounds() {
        let mut s = RowSelection::default();
        s.toggle(3, 10);
        s.toggle(5, 10);
        s.toggle(3, 10);
        s.toggle(-1, 10);
        s.toggle(10, 10);
        assert_eq!(rows(&s), [5]);
        assert!(s.contains(5) && !s.contains(3) && !s.contains(-1));
    }

    #[test]
    fn ranges_clamp_replace_or_extend() {
        let mut s = RowSelection::default();
        s.select_range(8, 2, false, 10);
        assert_eq!(rows(&s), [2, 3, 4, 5, 6, 7, 8]);
        s.select_range(-5, 1, false, 10);
        assert_eq!(rows(&s), [0, 1]);
        s.select_range(8, 50, true, 10);
        assert_eq!(rows(&s), [0, 1, 8, 9]);
        s.select_range(0, 3, false, 0);
        assert!(s.is_empty());
    }

    #[test]
    fn a_range_over_a_million_rows_is_cheap() {
        let mut s = RowSelection::default();
        s.select_range(0, i32::MAX, false, 1_456_457);
        assert_eq!(s.len(), 1_456_457);
        assert!(s.contains(1_456_456) && !s.contains(1_456_457));
        s.clear();
        assert_eq!(s.len(), 0);
    }
}
