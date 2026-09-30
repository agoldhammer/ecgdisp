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
    /// Record to show: a number (1 → 00001_hr), a record name (00001_hr),
    /// or a path to a .hea/.dat file.
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
pub fn record_header_path(record: &str, dir: &Path) -> PathBuf {
    let p = Path::new(record);
    let has_ext = matches!(p.extension().and_then(|e| e.to_str()), Some("hea" | "dat"));
    let is_path = record.contains(['/', '\\']);
    if is_path || has_ext {
        let p = if is_path {
            p.to_path_buf()
        } else {
            dir.join(p)
        };
        return p.with_extension("hea");
    }
    match record.parse::<u32>() {
        Ok(n) => dir.join(format!("{n:05}_hr.hea")),
        Err(_) => dir.join(format!("{record}.hea")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = "/data";

    fn resolve(r: &str) -> PathBuf {
        record_header_path(r, Path::new(DIR))
    }

    #[test]
    fn numbers_become_zero_padded_hr_records() {
        assert_eq!(resolve("1"), PathBuf::from("/data/00001_hr.hea"));
        assert_eq!(resolve("00042"), PathBuf::from("/data/00042_hr.hea"));
        assert_eq!(resolve("999"), PathBuf::from("/data/00999_hr.hea"));
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
