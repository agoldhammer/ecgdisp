//! The twelve standard ECG leads and command-line parsing for them.

use std::fmt;
use std::str::FromStr;

use crate::wfdb::{Lead, Record};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LeadName {
    I,
    II,
    III,
    AVR,
    AVL,
    AVF,
    V1,
    V2,
    V3,
    V4,
    V5,
    V6,
}

impl LeadName {
    /// All leads in the conventional display (and PTB-XL file) order.
    pub const ALL: [LeadName; 12] = [
        LeadName::I,
        LeadName::II,
        LeadName::III,
        LeadName::AVR,
        LeadName::AVL,
        LeadName::AVF,
        LeadName::V1,
        LeadName::V2,
        LeadName::V3,
        LeadName::V4,
        LeadName::V5,
        LeadName::V6,
    ];

    /// The name as it appears in PTB-XL headers.
    pub fn as_str(self) -> &'static str {
        match self {
            LeadName::I => "I",
            LeadName::II => "II",
            LeadName::III => "III",
            LeadName::AVR => "AVR",
            LeadName::AVL => "AVL",
            LeadName::AVF => "AVF",
            LeadName::V1 => "V1",
            LeadName::V2 => "V2",
            LeadName::V3 => "V3",
            LeadName::V4 => "V4",
            LeadName::V5 => "V5",
            LeadName::V6 => "V6",
        }
    }
}

impl fmt::Display for LeadName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for LeadName {
    type Err = String;

    /// Case-insensitive, so `aVR`, `avr` and `AVR` are all accepted.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        LeadName::ALL
            .into_iter()
            .find(|l| l.as_str().eq_ignore_ascii_case(s))
            .ok_or_else(|| {
                let valid: Vec<_> = LeadName::ALL.iter().map(|l| l.as_str()).collect();
                format!("unknown lead {s:?} (valid: {}, or ALL)", valid.join(", "))
            })
    }
}

/// Resolve the leads requested on the command line: an empty list or `ALL`
/// means all twelve; duplicates are dropped, keeping the first occurrence.
pub fn resolve(requested: &[String]) -> Result<Vec<LeadName>, String> {
    if requested.is_empty()
        || requested
            .iter()
            .any(|s| s.trim().eq_ignore_ascii_case("all"))
    {
        return Ok(LeadName::ALL.to_vec());
    }
    let mut leads = Vec::new();
    for s in requested {
        let lead: LeadName = s.parse()?;
        if !leads.contains(&lead) {
            leads.push(lead);
        }
    }
    Ok(leads)
}

/// Pick the requested leads out of a record, in the requested order.
pub fn select(record: &Record, leads: &[LeadName]) -> Result<Vec<Lead>, String> {
    leads
        .iter()
        .map(|l| {
            record.lead(l.as_str()).cloned().ok_or_else(|| {
                let have: Vec<_> = record.leads.iter().map(|l| l.name.as_str()).collect();
                format!(
                    "record {} has no lead {l} (has: {})",
                    record.name,
                    have.join(", ")
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(names: &[&str]) -> Record {
        Record {
            name: "r".into(),
            fs: 500.0,
            leads: names
                .iter()
                .map(|n| Lead {
                    name: n.to_string(),
                    units: "mV".into(),
                    samples: vec![0.0],
                })
                .collect(),
        }
    }

    #[test]
    fn select_returns_leads_in_requested_order() {
        let rec = record(&["I", "II", "AVR", "V1"]);
        let got = select(&rec, &[LeadName::V1, LeadName::AVR, LeadName::I]).unwrap();
        let names: Vec<_> = got.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["V1", "AVR", "I"]);
    }

    #[test]
    fn select_reports_missing_lead() {
        let err = select(&record(&["I"]), &[LeadName::V6]).unwrap_err();
        assert!(err.contains("no lead V6"), "{err}");
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_every_lead_case_insensitively() {
        for lead in LeadName::ALL {
            assert_eq!(lead.as_str().parse::<LeadName>(), Ok(lead));
            assert_eq!(lead.as_str().to_lowercase().parse::<LeadName>(), Ok(lead));
        }
        assert_eq!("aVF".parse::<LeadName>(), Ok(LeadName::AVF));
        assert_eq!(" v3 ".parse::<LeadName>(), Ok(LeadName::V3));
    }

    #[test]
    fn rejects_unknown_leads() {
        for bad in ["V7", "IV", "", "aV"] {
            let err = bad.parse::<LeadName>().unwrap_err();
            assert!(err.contains("unknown lead"), "{err}");
        }
    }

    #[test]
    fn resolve_defaults_to_all() {
        assert_eq!(resolve(&[]).unwrap(), LeadName::ALL.to_vec());
        assert_eq!(resolve(&strings(&["all"])).unwrap(), LeadName::ALL.to_vec());
    }

    #[test]
    fn resolve_keeps_order_and_drops_duplicates() {
        assert_eq!(
            resolve(&strings(&["V1", "ii", "V1", "avl"])).unwrap(),
            vec![LeadName::V1, LeadName::II, LeadName::AVL]
        );
    }

    #[test]
    fn resolve_propagates_errors() {
        assert!(resolve(&strings(&["I", "X"])).is_err());
    }

    #[test]
    fn display_matches_header_names() {
        assert_eq!(LeadName::AVR.to_string(), "AVR");
        assert_eq!(LeadName::V6.to_string(), "V6");
    }
}
