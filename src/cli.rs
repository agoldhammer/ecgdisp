//! Command-line arguments and record path resolution.

use std::path::{Path, PathBuf};

use clap::Parser;

/// Location of the PTB-XL 500 Hz records, relative to the home directory.
const DEFAULT_DATA_DIR: &str =
    "Prog/ekgdata/data/ptb/physionet.org/files/ptb-xl/1.0.3/records500/00000";

/// Display a 10-second, 12-lead PTB-XL ECG record.
#[derive(Debug, Parser)]
#[command(version)]
pub struct Args {
    /// Record to show: a number of up to three digits, combined with the first
    /// two characters of the directory name (1 in .../01000 → 01001_hr),
    /// a record name (00001_hr), or a path to a .hea/.dat file.
    #[arg(default_value = "1")]
    pub record: String,

    /// Leads to display, comma-separated and/or repeated:
    /// I, II, III, AVR, AVL, AVF, V1..V6, or "all" for all twelve (the default).
    /// Case-insensitive.
    #[arg(
        short,
        long,
        value_delimiter = ',',
        value_name = "LEADS",
        default_value = "all"
    )]
    pub leads: Vec<String>,

    /// Directory holding the records
    #[arg(short, long, default_value_os_t = default_data_dir())]
    pub dir: PathBuf,
}

pub fn default_data_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join(DEFAULT_DATA_DIR)
}

/// Turn the `record` argument into the path of its `.hea` file.
///
/// A plain number of at most three digits names a record in `dir`: the first
/// two characters of `dir`'s name followed by the number zero-padded to three
/// digits, so `5` in `.../01000` is `01005_hr`.
pub fn record_header_path(record: &str, dir: &Path) -> Result<PathBuf, String> {
    let p = Path::new(record);
    let has_ext = matches!(p.extension().and_then(|e| e.to_str()), Some("hea" | "dat"));
    let is_path = record.contains(['/', '\\']);
    if is_path || has_ext {
        let p = if is_path {
            p.to_path_buf()
        } else {
            dir.join(p)
        };
        return Ok(p.with_extension("hea"));
    }
    if !record.is_empty() && record.bytes().all(|b| b.is_ascii_digit()) {
        if record.len() > 3 {
            return Err(format!(
                "record number '{record}' has more than three digits"
            ));
        }
        let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let Some(prefix) = dir_name.get(..2) else {
            return Err(format!(
                "directory name '{dir_name}' is too short to give a record prefix"
            ));
        };
        return Ok(dir.join(format!("{prefix}{record:0>3}_hr.hea")));
    }
    Ok(dir.join(format!("{record}.hea")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = "/data";

    fn resolve(r: &str) -> PathBuf {
        record_header_path(r, Path::new(DIR)).unwrap()
    }

    fn resolve_in(r: &str, dir: &str) -> Result<PathBuf, String> {
        record_header_path(r, Path::new(dir))
    }

    #[test]
    fn numbers_combine_dir_prefix_with_three_padded_digits() {
        let ok = |r, dir, want| assert_eq!(resolve_in(r, dir), Ok(PathBuf::from(want)), "{r}");
        ok("1", "/r/00000", "/r/00000/00001_hr.hea");
        ok("42", "/r/00000", "/r/00000/00042_hr.hea");
        ok("999", "/r/00000", "/r/00000/00999_hr.hea");
        ok("5", "/r/01000", "/r/01000/01005_hr.hea");
        ok("007", "/r/21000", "/r/21000/21007_hr.hea");
        ok("123", "/r/21000/", "/r/21000/21123_hr.hea");
    }

    #[test]
    fn numbers_longer_than_three_digits_are_rejected() {
        for r in ["1000", "0001", "00042"] {
            let e = resolve_in(r, "/r/00000").unwrap_err();
            assert!(e.contains("more than three digits"), "{r}: {e}");
        }
    }

    #[test]
    fn numbers_need_a_two_character_dir_name() {
        assert!(resolve_in("1", "/r/0").unwrap_err().contains("too short"));
        assert!(resolve_in("1", "/").is_err());
    }

    #[test]
    fn names_and_file_names_resolve_in_dir() {
        assert_eq!(resolve("00007_hr"), PathBuf::from("/data/00007_hr.hea"));
        assert_eq!(resolve("00007_hr.hea"), PathBuf::from("/data/00007_hr.hea"));
        assert_eq!(resolve("00007_hr.dat"), PathBuf::from("/data/00007_hr.hea"));
    }

    #[test]
    fn paths_are_used_as_given() {
        assert_eq!(
            resolve("/x/y/00003_hr.dat"),
            PathBuf::from("/x/y/00003_hr.hea")
        );
        assert_eq!(resolve("rel/00003_hr"), PathBuf::from("rel/00003_hr.hea"));
    }

    #[test]
    fn parses_lead_option_forms() {
        let a = Args::try_parse_from(["ecgdisp", "5", "-l", "I,II", "--leads", "v1"]).unwrap();
        assert_eq!(a.record, "5");
        assert_eq!(a.leads, ["I", "II", "v1"]);
        let a = Args::try_parse_from(["ecgdisp"]).unwrap();
        assert_eq!(a.record, "1");
        assert_eq!(a.leads, ["all"]);
        assert_eq!(a.dir, default_data_dir());
    }

    #[test]
    fn lead_option_all_selects_every_lead() {
        use crate::leads::{LeadName, resolve};
        for argv in [
            &["ecgdisp"][..],
            &["ecgdisp", "-l", "all"],
            &["ecgdisp", "--leads", "ALL"],
        ] {
            let a = Args::try_parse_from(argv).unwrap();
            assert_eq!(
                resolve(&a.leads).unwrap(),
                LeadName::ALL.to_vec(),
                "{argv:?}"
            );
        }
    }

    #[test]
    fn default_dir_points_at_ptbxl_records() {
        assert!(default_data_dir().ends_with("records500/00000"));
    }
}
