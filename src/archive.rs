//! On-the-fly extraction of PTB-XL record folders from the dataset zip.
//!
//! The zip stores records as `<top>/records500/02000/02106_hr.{hea,dat}`.
//! A folder is unpacked whole (like `../ekgdata/extract.sh`) into the data
//! directory the first time one of its records is needed, so previous/next
//! navigation then works on disk.

use std::fs::{self, File};
use std::io::{self, Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use zip::ZipArchive;

/// The dataset zip, next to this project in `../ekgdata`, fixed at build time.
pub fn default_zip() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../ekgdata/ptb-xl-a-large-publicly-available-electrocardiography-dataset-1.0.3.zip")
}

/// Make sure the record at `hea` exists, extracting its folder from `zip`
/// if `hea` is missing and lies in a five-digit folder directly under `dir`
/// (`dir/02000/02106_hr.hea`). Other paths are left alone. Returns the number
/// of files extracted.
pub fn ensure_record(zip: &Path, dir: &Path, hea: &Path) -> Result<usize, String> {
    if hea.exists() {
        return Ok(0);
    }
    let Some(folder_path) = hea.parent() else {
        return Ok(0);
    };
    let folder = folder_path.file_name().and_then(|f| f.to_str());
    let Some(folder) = folder.filter(|f| is_folder_name(f)) else {
        return Ok(0);
    };
    if folder_path.parent() != Some(dir) || folder_path.exists() {
        return Ok(0);
    }
    let file = File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    extract_folder(file, folder, dir).map_err(|e| format!("{}: {e}", zip.display()))
}

fn is_folder_name(s: &str) -> bool {
    s.len() == 5 && s.bytes().all(|b| b.is_ascii_digit())
}

/// The file name of a zip entry `…/records500/<folder>/<name>.{hea,dat}`.
fn entry_file<'a>(entry: &'a str, folder: &str) -> Option<&'a str> {
    let mut parts = entry.rsplit('/');
    let name = parts.next()?;
    let ok = parts.next() == Some(folder)
        && parts.next() == Some("records500")
        && (name.ends_with(".hea") || name.ends_with(".dat"));
    ok.then_some(name)
}

/// Unpack the `.hea`/`.dat` files of `records500/<folder>` into `dir/<folder>`.
///
/// Files go to a scratch folder that is renamed into place at the end, so an
/// interrupted run never leaves a half-filled folder that looks complete.
pub fn extract_folder<R: Read + Seek>(reader: R, folder: &str, dir: &Path) -> io::Result<usize> {
    let mut archive = ZipArchive::new(reader)?;
    let target = dir.join(folder);
    static SCRATCH: AtomicUsize = AtomicUsize::new(0);
    let n = SCRATCH.fetch_add(1, Ordering::Relaxed);
    let scratch = dir.join(format!(".{folder}.partial-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch)?;
    let result = (|| {
        let mut count = 0;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let Some(name) = entry_file(entry.name(), folder) else {
                continue;
            };
            let mut out = File::create(scratch.join(name))?;
            io::copy(&mut entry, &mut out)?;
            count += 1;
        }
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no records500/{folder} records in the archive"),
            ));
        }
        Ok(count)
    })();
    match result {
        // Another process may have extracted the same folder meanwhile; keep theirs.
        Ok(count) => match fs::rename(&scratch, &target) {
            Ok(()) => Ok(count),
            Err(_) if target.is_dir() => {
                let _ = fs::remove_dir_all(&scratch);
                Ok(count)
            }
            Err(e) => {
                let _ = fs::remove_dir_all(&scratch);
                Err(e)
            }
        },
        Err(e) => {
            let _ = fs::remove_dir_all(&scratch);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::{SimpleFileOptions, ZipWriter};

    const TOP: &str = "ptb-xl-1.0.3";

    fn archive(files: &[&str]) -> Vec<u8> {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        for f in files {
            w.start_file(format!("{TOP}/{f}"), SimpleFileOptions::default())
                .unwrap();
            w.write_all(f.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ecgdisp-archive-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut v: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    const FILES: [&str; 7] = [
        "RECORDS",
        "records500/00000/00001_hr.hea",
        "records500/00000/00001_hr.dat",
        "records500/00000/index.html",
        "records500/01000/01000_hr.hea",
        "records100/00000/00001_lr.hea",
        "records500/00000/sub/00009_hr.hea",
    ];

    #[test]
    fn entry_names_must_sit_in_the_records500_folder() {
        assert_eq!(
            entry_file("t/records500/00000/00001_hr.hea", "00000"),
            Some("00001_hr.hea")
        );
        assert_eq!(entry_file("t/records500/00000/index.html", "00000"), None);
        assert_eq!(entry_file("t/records500/01000/01000_hr.hea", "00000"), None);
        assert_eq!(entry_file("t/records100/00000/00001_lr.hea", "00000"), None);
        assert_eq!(entry_file("t/records500/00000/", "00000"), None);
    }

    #[test]
    fn extracts_only_the_folders_headers_and_data() {
        let dir = temp_dir("one");
        let n = extract_folder(Cursor::new(archive(&FILES)), "00000", &dir).unwrap();
        assert_eq!(n, 2);
        assert_eq!(listing(&dir), ["00000"]);
        assert_eq!(
            listing(&dir.join("00000")),
            ["00001_hr.dat", "00001_hr.hea"]
        );
        let hea = fs::read_to_string(dir.join("00000/00001_hr.hea")).unwrap();
        assert_eq!(hea, "records500/00000/00001_hr.hea");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_folder_is_an_error_and_leaves_nothing_behind() {
        let dir = temp_dir("missing");
        let e = extract_folder(Cursor::new(archive(&FILES)), "05000", &dir).unwrap_err();
        assert!(e.to_string().contains("records500/05000"), "{e}");
        assert!(listing(&dir).is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ensure_record_extracts_once_and_only_for_data_dir_records() {
        let dir = temp_dir("ensure");
        let zip = dir.join("data.zip");
        fs::write(&zip, archive(&FILES)).unwrap();
        let data = dir.join("records500");
        fs::create_dir_all(&data).unwrap();
        let hea = data.join("00000/00001_hr.hea");
        assert_eq!(ensure_record(&zip, &data, &hea), Ok(2));
        assert!(hea.is_file());
        assert_eq!(ensure_record(&zip, &data, &hea), Ok(0));
        // A record missing from an extracted folder does not re-extract it.
        assert_eq!(
            ensure_record(&zip, &data, &data.join("00000/00002_hr.hea")),
            Ok(0)
        );
        // Paths outside the data directory are not ours to fill.
        assert_eq!(
            ensure_record(&zip, &data, &dir.join("01000/01000_hr.hea")),
            Ok(0)
        );
        assert_eq!(ensure_record(&zip, &data, &data.join("odd.hea")), Ok(0));
        let e = ensure_record(
            &dir.join("nope.zip"),
            &data,
            &data.join("01000/01000_hr.hea"),
        )
        .unwrap_err();
        assert!(e.contains("nope.zip"), "{e}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
