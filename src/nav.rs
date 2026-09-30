//! The ordered list of records in a directory, for previous/next navigation.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct RecordList {
    /// `.hea` paths sorted by file name.
    paths: Vec<PathBuf>,
    current: usize,
}

impl RecordList {
    /// All `.hea` files in `start`'s directory, positioned at `start`.
    /// `start` is included even if the directory cannot be listed.
    pub fn scan(start: &Path) -> Self {
        let dir = start.parent().unwrap_or(Path::new("."));
        let paths = fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "hea"))
                    .collect()
            })
            .unwrap_or_default();
        Self::new(paths, start)
    }

    pub fn new(mut paths: Vec<PathBuf>, start: &Path) -> Self {
        let key = |p: &Path| p.file_name().map(|n| n.to_owned());
        if !paths.iter().any(|p| key(p) == key(start)) {
            paths.push(start.to_path_buf());
        }
        paths.sort_by_key(|p| key(p));
        let current = paths.iter().position(|p| key(p) == key(start)).unwrap_or(0);
        Self { paths, current }
    }

    pub fn current(&self) -> &Path {
        &self.paths[self.current]
    }

    /// 1-based position and total count, e.g. `(3, 987)`.
    pub fn position(&self) -> (usize, usize) {
        (self.current + 1, self.paths.len())
    }

    /// Records from the one after the current in direction `step` (±1) to
    /// the end of the list, nearest first.
    pub fn candidates(&self, step: isize) -> Vec<(usize, &Path)> {
        let order: Box<dyn Iterator<Item = usize>> = if step < 0 {
            Box::new((0..self.current).rev())
        } else {
            Box::new(self.current + 1..self.paths.len())
        };
        order.map(|i| (i, self.paths[i].as_path())).collect()
    }

    pub fn set_current(&mut self, index: usize) {
        assert!(index < self.paths.len());
        self.current = index;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(names: &[&str], start: &str) -> RecordList {
        let paths = names.iter().map(|n| PathBuf::from("/d").join(n)).collect();
        RecordList::new(paths, &PathBuf::from("/d").join(start))
    }

    fn names(c: Vec<(usize, &Path)>) -> Vec<String> {
        c.iter()
            .map(|(_, p)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn sorts_and_positions_at_start() {
        let l = list(
            &["00003_hr.hea", "00001_hr.hea", "00002_hr.hea"],
            "00002_hr.hea",
        );
        assert_eq!(l.current(), Path::new("/d/00002_hr.hea"));
        assert_eq!(l.position(), (2, 3));
    }

    #[test]
    fn candidates_go_outward_from_current() {
        let l = list(&["a.hea", "b.hea", "c.hea", "d.hea"], "b.hea");
        assert_eq!(names(l.candidates(1)), ["c.hea", "d.hea"]);
        assert_eq!(names(l.candidates(-1)), ["a.hea"]);
        assert_eq!(l.candidates(1)[0].0, 2);
    }

    #[test]
    fn no_candidates_past_the_ends() {
        let l = list(&["a.hea", "b.hea"], "a.hea");
        assert!(l.candidates(-1).is_empty());
        let l = list(&["a.hea", "b.hea"], "b.hea");
        assert!(l.candidates(1).is_empty());
    }

    #[test]
    fn start_is_added_when_missing_from_listing() {
        let l = list(&["a.hea", "c.hea"], "b.hea");
        assert_eq!(l.position(), (2, 3));
        let empty = RecordList::new(vec![], Path::new("/x/r.hea"));
        assert_eq!(empty.position(), (1, 1));
        assert!(empty.candidates(1).is_empty());
    }

    #[test]
    fn set_current_moves_position() {
        let mut l = list(&["a.hea", "b.hea", "c.hea"], "a.hea");
        l.set_current(2);
        assert_eq!(l.current(), Path::new("/d/c.hea"));
    }

    #[test]
    fn scan_lists_only_header_files() {
        let dir = std::env::temp_dir().join(format!("ecgdisp-nav-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for f in ["2.hea", "2.dat", "1.hea", "notes.txt", "3.hea"] {
            fs::write(dir.join(f), "").unwrap();
        }
        let l = RecordList::scan(&dir.join("2.hea"));
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(l.position(), (2, 3));
        assert_eq!(names(l.candidates(1)), ["3.hea"]);
    }
}
