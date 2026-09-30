//! Checks against the real PTB-XL records; skipped when the dataset is absent.

use ecgdisp::{cli, leads, wfdb};

fn load(record: &str) -> Option<wfdb::Record> {
    let hea = cli::record_header_path(record, &cli::default_data_dir());
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
    let dir = cli::default_data_dir();
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
