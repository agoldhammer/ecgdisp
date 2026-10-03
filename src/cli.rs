//! Command-line arguments and record path resolution.

use std::path::{Path, PathBuf};

use clap::Parser;

use crate::archive;

/// Where record folders unpacked from the dataset zip go, under the system
/// temporary directory (`/tmp`). Records live in subdirectories of a thousand
/// each: `00000`, `01000`, …
const DEFAULT_DATA_DIR: &str = "ecgdisp/records500";

/// `ptbxl_database.xlsx` relative to the project directory (the zip only
/// carries the `.csv` version).
const DEFAULT_DATABASE: &str =
    "../ekgdata/data/ptb/physionet.org/files/ptb-xl/1.0.3/ptbxl_database.xlsx";

/// Display a 10-second, 12-lead PTB-XL ECG record.
#[derive(Debug, Parser)]
#[command(version)]
pub struct Args {
    /// Record to show: a number of up to five digits (2106 → 02000/02106_hr),
    /// a record name (02106_hr), or a path to a .hea/.dat file.
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

    /// Base directory holding the record subdirectories (00000, 01000, …).
    /// A missing subdirectory is unpacked into it from --zip on first use.
    #[arg(short, long, default_value_os_t = default_data_dir())]
    pub dir: PathBuf,

    /// PTB-XL dataset zip to unpack record folders from
    #[arg(short, long, default_value_os_t = archive::default_zip())]
    pub zip: PathBuf,

    /// Record spreadsheet, used when none is found above the record
    #[arg(long, value_name = "XLSX", default_value_os_t = default_database())]
    pub db: PathBuf,
}

/// `ecgdisp/records500` in the system temporary directory.
pub fn default_data_dir() -> PathBuf {
    std::env::temp_dir().join(DEFAULT_DATA_DIR)
}

/// `ptbxl_database.xlsx` next to the project, fixed at build time so the
/// installed binary finds it from any working directory.
pub fn default_database() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(DEFAULT_DATABASE)
}

/// Turn the `record` argument into the path of its `.hea` file.
///
/// A plain number of at most five digits is zero-padded to five; its first two
/// digits followed by `000` name the subdirectory of `dir`, so `2106` is
/// `dir/02000/02106_hr.hea`. A record name whose leading digits form such a
/// number (`02106_hr`) goes to the same subdirectory.
pub fn record_header_path(record: &str, dir: &Path) -> Result<PathBuf, String> {
    let p = Path::new(record);
    if record.contains(['/', '\\']) {
        return Ok(p.with_extension("hea"));
    }
    if !record.is_empty() && record.bytes().all(|b| b.is_ascii_digit()) {
        if record.len() > 5 {
            return Err(format!(
                "record number '{record}' has more than five digits"
            ));
        }
        return Ok(in_subdir(dir, &format!("{record:0>5}_hr")));
    }
    let name = match p.extension().and_then(|e| e.to_str()) {
        Some("hea" | "dat") => p.with_extension(""),
        _ => p.to_path_buf(),
    };
    Ok(in_subdir(dir, &name.to_string_lossy()))
}

/// The `.hea` path of record `name` in its thousands subdirectory of `dir`
/// (`02106_hr` → `dir/02000/02106_hr.hea`), or directly in `dir` if the name
/// does not start with five digits.
fn in_subdir(dir: &Path, name: &str) -> PathBuf {
    let file = format!("{name}.hea");
    match name.get(..5) {
        Some(d) if d.bytes().all(|b| b.is_ascii_digit()) => {
            dir.join(format!("{}000", &d[..2])).join(file)
        }
        _ => dir.join(file),
    }
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
    fn numbers_pad_to_five_digits_in_thousands_subdir() {
        let ok = |r, want| assert_eq!(resolve(r), PathBuf::from(want), "{r}");
        ok("1", "/data/00000/00001_hr.hea");
        ok("134", "/data/00000/00134_hr.hea");
        ok("999", "/data/00000/00999_hr.hea");
        ok("1000", "/data/01000/01000_hr.hea");
        ok("2106", "/data/02000/02106_hr.hea");
        ok("21837", "/data/21000/21837_hr.hea");
        ok("00042", "/data/00000/00042_hr.hea");
        assert_eq!(
            resolve_in("2106", "/r/"),
            Ok(PathBuf::from("/r/02000/02106_hr.hea"))
        );
    }

    #[test]
    fn numbers_longer_than_five_digits_are_rejected() {
        for r in ["100000", "000001"] {
            let e = resolve_in(r, "/r").unwrap_err();
            assert!(e.contains("more than five digits"), "{r}: {e}");
        }
    }

    #[test]
    fn names_and_file_names_resolve_in_their_subdir() {
        assert_eq!(
            resolve("02106_hr"),
            PathBuf::from("/data/02000/02106_hr.hea")
        );
        assert_eq!(
            resolve("00007_hr.hea"),
            PathBuf::from("/data/00000/00007_hr.hea")
        );
        assert_eq!(
            resolve("00007_hr.dat"),
            PathBuf::from("/data/00000/00007_hr.hea")
        );
        assert_eq!(resolve("odd"), PathBuf::from("/data/odd.hea"));
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
        assert_eq!(a.zip, archive::default_zip());
        let a = Args::try_parse_from(["ecgdisp", "-z", "/x.zip", "--db", "/d.xlsx"]).unwrap();
        assert_eq!(a.zip, PathBuf::from("/x.zip"));
        assert_eq!(a.db, PathBuf::from("/d.xlsx"));
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
        let d = default_data_dir();
        assert!(d.is_absolute(), "{}", d.display());
        assert!(d.starts_with(std::env::temp_dir()), "{}", d.display());
        assert!(d.ends_with("ecgdisp/records500"), "{}", d.display());
        assert!(default_database().ends_with("1.0.3/ptbxl_database.xlsx"));
        assert!(archive::default_zip().is_absolute());
    }
}
