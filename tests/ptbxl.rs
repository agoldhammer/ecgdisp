//! Checks against the real PTB-XL records, unpacked on demand from the dataset
//! zip; skipped when the dataset is absent.

use std::path::PathBuf;

use ecgdisp::{analysis, archive, cli, database, leads, wfdb};

/// The `.hea` path of `record`, unpacking its folder from the zip if needed.
fn header(record: &str) -> PathBuf {
    let dir = cli::default_data_dir();
    let hea = cli::record_header_path(record, &dir).unwrap();
    let zip = archive::default_zip();
    if zip.is_file() {
        archive::ensure_record(&zip, &dir, &hea).expect("folder should unpack");
    }
    hea
}

fn load(record: &str) -> Option<wfdb::Record> {
    let hea = header(record);
    if !hea.exists() {
        eprintln!("skipping: {} not found", hea.display());
        return None;
    }
    Some(wfdb::load_record(&hea).expect("record should load"))
}

#[test]
fn record_00001_is_ten_seconds_of_twelve_leads() {
    let Some(rec) = load("1") else { return };
    assert_eq!(rec.name, "00001_hr");
    assert_eq!(rec.fs, 500.0);
    assert_eq!(rec.n_samples(), 5000);
    assert_eq!(rec.duration_s(), 10.0);
    let names: Vec<_> = rec.leads.iter().map(|l| l.name.as_str()).collect();
    let all: Vec<_> = leads::LeadName::ALL.iter().map(|l| l.as_str()).collect();
    assert_eq!(names, all);
    // Header initial value for lead I is -115 at 1000 units/mV.
    assert_eq!(rec.lead("I").unwrap().samples[0], -0.115);
    assert!(
        rec.leads
            .iter()
            .all(|l| l.samples.iter().all(|v| v.is_finite() && v.abs() < 20.0))
    );
}

#[test]
fn every_record_in_the_directory_loads() {
    let dir = header("1").parent().unwrap().to_path_buf();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("skipping: {} not found", dir.display());
        return;
    };
    let mut count = 0;
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "hea") {
            let rec = wfdb::load_record(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            assert_eq!(rec.leads.len(), 12, "{}", p.display());
            count += 1;
        }
    }
    assert!(count > 0);
}

#[test]
fn five_digit_numbers_find_records_in_later_subdirs() {
    let Some(rec) = load("2106") else { return };
    assert_eq!(rec.name, "02106_hr");
    assert_eq!(rec.leads.len(), 12);
}

#[test]
fn database_row_for_record_00001() {
    let hea = header("1");
    let fallback = cli::default_database();
    let found = database::find(&hea).or_else(|| fallback.is_file().then_some(fallback));
    let Some(path) = found else {
        eprintln!("skipping: {} not found", database::FILE_NAME);
        return;
    };
    let start = std::time::Instant::now();
    let db = database::Database::open(&path).expect("spreadsheet should load");
    eprintln!("loaded {} rows in {:?}", db.len(), start.elapsed());
    assert_eq!(db.len(), 21799);
    let info = db.get(database::ecg_id(&hea).unwrap()).unwrap();
    assert_eq!(info.report, "sinusrhythmus periphere niederspannung");
    assert_eq!(info.scp_codes, "{'NORM': 100.0, 'LVOLT': 0.0, 'SR': 0.0}");
    assert_eq!(info.heart_axis, "");
}

/// Values printed by `ekg-analyze` (../ekgdata) for the same records.
#[test]
fn analysis_matches_the_python_analyzer() {
    let round = |v: f64| v.round() as i64;
    for (record, beats, hr, rr_sd, pr, qrs, wide, qt, qtc, axis, lvh) in [
        (
            "1",
            11,
            64,
            16,
            70,
            152,
            false,
            436,
            450,
            18,
            [1.23, 0.97, 0.42],
        ),
        (
            "984",
            10,
            61,
            34,
            138,
            92,
            false,
            394,
            400,
            -24,
            [2.21, 2.30, 1.21],
        ),
        (
            "180",
            15,
            93,
            7,
            156,
            190,
            true,
            372,
            462,
            -23,
            [4.47, 6.14, 1.18],
        ),
    ] {
        let Some(rec) = load(record) else { return };
        let a = analysis::analyze(&rec).unwrap();
        let iv = a.intervals.as_ref().unwrap();
        let l = a.lvh.as_ref().unwrap();
        assert_eq!(a.qrs.len(), beats, "{record}");
        assert_eq!(round(a.hr_bpm), hr, "{record}");
        assert_eq!(round(a.rr_sd_ms), rr_sd, "{record}");
        assert_eq!(iv.pr_ms.map(round), Some(pr), "{record}");
        assert_eq!((round(iv.qrs_ms), iv.qrs_wide), (qrs, wide), "{record}");
        assert_eq!(round(iv.qt_ms), qt, "{record}");
        assert_eq!(round(iv.qtc_bazett_ms), qtc, "{record}");
        assert_eq!(round(a.qrs_axis_deg.unwrap()), axis, "{record}");
        let got = [l.sokolow_lyon, l.cornell, l.ravl].map(|v| (v * 100.0).round() / 100.0);
        assert_eq!(got, lvh, "{record}");
    }
}

#[test]
fn record_180_is_flagged_for_wide_qrs_and_lvh_voltage() {
    let Some(rec) = load("180") else { return };
    let a = analysis::analyze(&rec).unwrap();
    assert!(
        a.summary()
            .iter()
            .any(|i| i.text == "QRS ≥190 ms" && i.alert)
    );
    let v = a.voltage(None);
    assert!(v[0].text.starts_with("Sokolow-Lyon") && v[0].alert);
    assert!(v.iter().any(|i| i.text.contains("700 ms beat window")));
}
