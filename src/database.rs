//! Per-record metadata from the PTB-XL spreadsheet `ptbxl_database.xlsx`.
//!
//! Rows are keyed by `ecg_id` (column A), which is the five-digit record
//! number (`01005_hr` → 1005).
//! Columns K, L and M hold the report, the SCP codes and the heart axis;
//! column D holds the patient's sex (0 = male, 1 = female).

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use calamine::{Data, Reader, Xlsx, open_workbook};

use crate::analysis::Sex;

pub const FILE_NAME: &str = "ptbxl_database.xlsx";

/// Zero-based spreadsheet columns: A = ecg_id, D = sex, K = report, L = scp_codes, M = heart_axis.
const ID_COL: usize = 0;
const SEX_COL: usize = 3;
const INFO_COLS: [usize; 3] = [10, 11, 12];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RecordInfo {
    pub report: String,
    pub scp_codes: String,
    pub heart_axis: String,
    pub sex: Option<Sex>,
}

impl fmt::Display for RecordInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  report:     {}", self.report)?;
        writeln!(f, "  scp_codes:  {}", self.scp_codes)?;
        write!(f, "  heart_axis: {}", self.heart_axis)
    }
}

#[derive(Debug, Default)]
pub struct Database {
    rows: HashMap<u32, RecordInfo>,
}

impl Database {
    /// Read the first worksheet of the spreadsheet at `path`.
    pub fn open(path: &Path) -> Result<Self, String> {
        let err = |e: &dyn fmt::Display| format!("{}: {e}", path.display());
        let mut book: Xlsx<_> = open_workbook(path).map_err(|e| err(&e))?;
        let range = book
            .worksheet_range_at(0)
            .ok_or_else(|| err(&"no worksheet"))?
            .map_err(|e| err(&e))?;
        // Cells are addressed relative to the used range; shift to absolute columns.
        let (_, first_col) = range.start().unwrap_or((0, 0));
        let first_col = first_col as usize;
        let rows = range.rows().map(|row| {
            (0..=INFO_COLS[2]).map(|col| {
                col.checked_sub(first_col)
                    .and_then(|i| row.get(i))
                    .unwrap_or(&Data::Empty)
            })
        });
        Ok(Self::from_rows(rows))
    }

    /// Build from rows of cells starting at column A. Rows whose column A is
    /// not a whole number (the header, blanks) are ignored.
    pub fn from_rows<'a, R, C>(rows: R) -> Self
    where
        R: IntoIterator<Item = C>,
        C: IntoIterator<Item = &'a Data>,
    {
        let mut map = HashMap::new();
        for row in rows {
            let cells: Vec<&Data> = row.into_iter().collect();
            let cell = |i: usize| cells.get(i).copied().unwrap_or(&Data::Empty);
            let Some(id) = cell_id(cell(ID_COL)) else {
                continue;
            };
            let [report, scp_codes, heart_axis] = INFO_COLS.map(|c| cell_text(cell(c)));
            map.insert(
                id,
                RecordInfo {
                    report,
                    scp_codes,
                    heart_axis,
                    sex: cell_sex(cell(SEX_COL)),
                },
            );
        }
        Self { rows: map }
    }

    pub fn get(&self, ecg_id: u32) -> Option<&RecordInfo> {
        self.rows.get(&ecg_id)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

fn cell_id(d: &Data) -> Option<u32> {
    match d {
        Data::Int(i) => u32::try_from(*i).ok(),
        Data::Float(f) if f.fract() == 0.0 && *f >= 0.0 && *f <= u32::MAX as f64 => Some(*f as u32),
        Data::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn cell_sex(d: &Data) -> Option<Sex> {
    let v = match d {
        Data::Int(i) => *i as f64,
        Data::Float(f) => *f,
        Data::String(s) => s.trim().parse().ok()?,
        _ => return None,
    };
    match v {
        0.0 => Some(Sex::Male),
        1.0 => Some(Sex::Female),
        _ => None,
    }
}

fn cell_text(d: &Data) -> String {
    match d {
        Data::Empty => String::new(),
        d => fix_mojibake(d.to_string().trim()),
    }
}

/// The byte Windows-1252 encodes `c` as, if any. Its five undefined bytes
/// (0x81, 0x8D, 0x8F, 0x90, 0x9D) pass through as the same code points.
fn cp1252_byte(c: char) -> Option<u8> {
    const HIGH: [(char, u8); 27] = [
        ('€', 0x80),
        ('‚', 0x82),
        ('ƒ', 0x83),
        ('„', 0x84),
        ('…', 0x85),
        ('†', 0x86),
        ('‡', 0x87),
        ('ˆ', 0x88),
        ('‰', 0x89),
        ('Š', 0x8A),
        ('‹', 0x8B),
        ('Œ', 0x8C),
        ('Ž', 0x8E),
        ('\u{2018}', 0x91),
        ('\u{2019}', 0x92),
        ('\u{201C}', 0x93),
        ('\u{201D}', 0x94),
        ('•', 0x95),
        ('–', 0x96),
        ('—', 0x97),
        ('˜', 0x98),
        ('™', 0x99),
        ('š', 0x9A),
        ('›', 0x9B),
        ('œ', 0x9C),
        ('ž', 0x9E),
        ('Ÿ', 0x9F),
    ];
    match c as u32 {
        0..=0x7F | 0xA0..=0xFF | 0x81 | 0x8D | 0x8F..=0x90 | 0x9D => Some(c as u8),
        _ => HIGH.iter().find(|&&(h, _)| h == c).map(|&(_, b)| b),
    }
}

/// Undo UTF-8 text that was decoded as Windows-1252, as in the PTB-XL
/// spreadsheet's reports (`vÃ„nster` → `vÄnster`). Text that does not
/// round-trip to valid UTF-8 is returned unchanged.
fn fix_mojibake(s: &str) -> String {
    if s.is_ascii() {
        return s.to_owned();
    }
    s.chars()
        .map(cp1252_byte)
        .collect::<Option<Vec<u8>>>()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_else(|| s.to_owned())
}

/// The `ecg_id` of the record whose header is `hea`: the leading digits of
/// its name (`.../01000/01005_hr.hea` → 1005).
pub fn ecg_id(hea: &Path) -> Option<u32> {
    let stem = hea.file_stem()?.to_str()?;
    let digits = stem.split(|c: char| !c.is_ascii_digit()).next()?;
    digits.parse().ok()
}

/// Find `ptbxl_database.xlsx` in the record's directory or any directory above
/// it (normally three levels up: `1.0.3/records500/00000/00001_hr.hea`).
pub fn find(hea: &Path) -> Option<PathBuf> {
    hea.ancestors()
        .skip(1)
        .map(|d| d.join(FILE_NAME))
        .find(|p| p.is_file())
}

/// The text printed to the terminal for the record at `hea`.
pub fn describe(db: &Database, hea: &Path) -> String {
    let name = hea
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    match ecg_id(hea) {
        None => format!("{name}: cannot derive ecg_id from the record name"),
        Some(id) => match db.get(id) {
            Some(info) => format!("{name} (ecg_id {id}):\n{info}"),
            None => format!("{name}: ecg_id {id} not found in {FILE_NAME}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> Data {
        Data::String(t.into())
    }

    /// A row with `id` in A and `k`, `l`, `m` in K, L, M.
    fn row(id: Data, k: Data, l: Data, m: Data) -> Vec<Data> {
        let mut r = vec![Data::Empty; 13];
        r[0] = id;
        r[10] = k;
        r[11] = l;
        r[12] = m;
        r
    }

    fn db(rows: &[Vec<Data>]) -> Database {
        Database::from_rows(rows.iter().map(|r| r.iter()))
    }

    #[test]
    fn ecg_id_comes_from_dir_prefix_and_number() {
        let id = |p: &str| ecg_id(Path::new(p));
        assert_eq!(id("/r/00000/00001_hr.hea"), Some(1));
        assert_eq!(id("/r/01000/01005_hr.hea"), Some(1005));
        assert_eq!(id("/r/21000/21837_hr.hea"), Some(21837));
        assert_eq!(id("/r/x/hr_00001.hea"), None);
    }

    #[test]
    fn ecg_id_matches_resolved_record_number() {
        let hea = crate::cli::record_header_path("1005", Path::new("/r")).unwrap();
        assert_eq!(ecg_id(&hea), Some(1005));
    }

    #[test]
    fn rows_are_keyed_by_column_a_and_read_k_l_m() {
        let d = db(&[
            row(s("ecg_id"), s("report"), s("scp_codes"), s("heart_axis")),
            row(
                Data::Int(1),
                s("sinusrhythmus"),
                s("{'NORM': 100.0}"),
                Data::Empty,
            ),
            row(
                Data::Float(1005.0),
                s(" lvh "),
                s("{'LVH': 50.0}"),
                s("LAD"),
            ),
        ]);
        assert_eq!(d.len(), 2);
        assert_eq!(
            d.get(1),
            Some(&RecordInfo {
                report: "sinusrhythmus".into(),
                scp_codes: "{'NORM': 100.0}".into(),
                heart_axis: String::new(),
                sex: None,
            })
        );
        let r = d.get(1005).unwrap();
        assert_eq!((r.report.as_str(), r.heart_axis.as_str()), ("lvh", "LAD"));
        assert_eq!(r.sex, None);
        assert!(d.get(2).is_none());
    }

    #[test]
    fn short_rows_and_non_numeric_ids_are_tolerated() {
        let d = db(&[
            vec![Data::Int(7), s("b")],
            vec![s("abc")],
            vec![Data::Float(1.5)],
            vec![],
        ]);
        assert_eq!(d.len(), 1);
        assert_eq!(d.get(7), Some(&RecordInfo::default()));
    }

    #[test]
    fn sex_comes_from_column_d() {
        let with_sex = |sex: Data| {
            let mut r = row(Data::Int(1), s("x"), s("y"), s("z"));
            r[3] = sex;
            db(&[r]).get(1).unwrap().sex
        };
        assert_eq!(with_sex(Data::Float(0.0)), Some(Sex::Male));
        assert_eq!(with_sex(Data::Int(1)), Some(Sex::Female));
        assert_eq!(with_sex(s("1")), Some(Sex::Female));
        assert_eq!(with_sex(Data::Float(2.0)), None);
        assert_eq!(with_sex(Data::Empty), None);
    }

    #[test]
    fn mojibake_is_repaired() {
        // Report texts as found in ptbxl_database.xlsx (ecg_id 49, 180, 18, 2284).
        assert_eq!(
            fix_mojibake("intraventrikulÃ„re leitungsstÃ–rung"),
            "intraventrikulÄre leitungsstÖrung"
        );
        assert_eq!(
            fix_mojibake("sinusrytm vÃ„nster el-axel vÃ„nstersidigt skÃ„nkelblock"),
            "sinusrytm vÄnster el-axel vÄnstersidigt skÄnkelblock"
        );
        assert_eq!(fix_mojibake("Ãœberleitung"), "Überleitung");
        assert_eq!(
            fix_mojibake("auszuschlieÃŸen erhÃ¶hte"),
            "auszuschließen erhöhte"
        );
        // Correct text, and text that is not mojibake, are left alone.
        assert_eq!(fix_mojibake("vänster"), "vänster");
        assert_eq!(fix_mojibake("Ã alone"), "Ã alone");
        assert_eq!(fix_mojibake("40° → 50°"), "40° → 50°");
        assert_eq!(cell_text(&s(" sÃ…som ")), "sÅsom");
    }

    #[test]
    fn describe_prints_the_three_columns_or_why_not() {
        let d = db(&[row(Data::Int(3), s("normal"), s("{'SR': 0.0}"), s("MID"))]);
        let text = describe(&d, Path::new("/r/00000/00003_hr.hea"));
        assert_eq!(
            text,
            "00003_hr (ecg_id 3):\n  report:     normal\n  scp_codes:  {'SR': 0.0}\n  heart_axis: MID"
        );
        assert!(describe(&d, Path::new("/r/00000/00004_hr.hea")).contains("not found"));
        assert!(describe(&d, Path::new("/r/x.hea")).contains("cannot derive"));
    }

    #[test]
    fn find_searches_parent_directories() {
        let root = std::env::temp_dir().join(format!("ecgdisp-db-{}", std::process::id()));
        let recs = root.join("records500/00000");
        std::fs::create_dir_all(&recs).unwrap();
        std::fs::write(root.join(FILE_NAME), "").unwrap();
        let found = find(&recs.join("00001_hr.hea"));
        let missing = find(Path::new("/nonexistent/a/b.hea"));
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(found, Some(root.join(FILE_NAME)));
        assert_eq!(missing, None);
    }
}
