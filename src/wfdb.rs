//! Minimal reader for PhysioNet WFDB records (`.hea` header + format-16 `.dat`),
//! as used by the PTB-XL dataset.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Raw ADC value WFDB uses to mark a missing sample in format 16.
const INVALID_SAMPLE: i16 = i16::MIN;

#[derive(Debug)]
pub enum WfdbError {
    Io(PathBuf, std::io::Error),
    Header(String),
    Data(String),
}

impl fmt::Display for WfdbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WfdbError::Io(path, e) => write!(f, "{}: {e}", path.display()),
            WfdbError::Header(msg) => write!(f, "invalid header: {msg}"),
            WfdbError::Data(msg) => write!(f, "invalid signal data: {msg}"),
        }
    }
}

impl std::error::Error for WfdbError {}

/// One signal line of a WFDB header.
#[derive(Debug, Clone, PartialEq)]
pub struct SignalSpec {
    pub file: String,
    pub format: u32,
    /// ADC units per physical unit (e.g. 1000 per mV).
    pub gain: f64,
    pub baseline: i32,
    pub units: String,
    pub init_value: Option<i32>,
    pub checksum: Option<i32>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub record_name: String,
    pub fs: f64,
    pub n_samples: usize,
    pub signals: Vec<SignalSpec>,
}

/// A decoded lead: samples in physical units (mV). Missing samples are NaN.
#[derive(Debug, Clone, PartialEq)]
pub struct Lead {
    pub name: String,
    pub units: String,
    pub samples: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub name: String,
    pub fs: f64,
    pub leads: Vec<Lead>,
}

impl Record {
    pub fn n_samples(&self) -> usize {
        self.leads.first().map_or(0, |l| l.samples.len())
    }

    pub fn duration_s(&self) -> f64 {
        self.n_samples() as f64 / self.fs
    }

    /// Find a lead by name, ignoring ASCII case ("aVR" matches "AVR").
    pub fn lead(&self, name: &str) -> Option<&Lead> {
        self.leads
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name))
    }
}

/// The leading numeric part of a WFDB field, dropping suffixes such as the
/// counter frequency in `500/1000` or the byte offset in `16+24`.
fn leading_number(field: &str) -> &str {
    let end = field
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || (i == 0 && c == '-')))
        .map_or(field.len(), |(i, _)| i);
    &field[..end]
}

fn parse_num<T: std::str::FromStr>(field: &str, what: &str) -> Result<T, WfdbError> {
    leading_number(field)
        .parse()
        .map_err(|_| WfdbError::Header(format!("bad {what}: {field:?}")))
}

/// Parse a gain field such as `1000.0(0)/mV`, `200/mV` or `1000`.
/// Returns (gain, baseline if given, units).
fn parse_gain(field: &str) -> Result<(f64, Option<i32>, String), WfdbError> {
    let (value, units) = match field.split_once('/') {
        Some((v, u)) => (v, u.to_string()),
        None => (field, "mV".to_string()),
    };
    let (gain_str, baseline) = match value.split_once('(') {
        Some((g, rest)) => {
            let b = rest
                .strip_suffix(')')
                .ok_or_else(|| WfdbError::Header(format!("bad baseline in {field:?}")))?;
            (g, Some(parse_num::<i32>(b, "baseline")?))
        }
        None => (value, None),
    };
    let mut gain: f64 = parse_num(gain_str, "gain")?;
    if gain == 0.0 {
        gain = 200.0; // WFDB default
    }
    Ok((gain, baseline, units))
}

pub fn parse_header(text: &str) -> Result<Header, WfdbError> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'));

    let record_line = lines
        .next()
        .ok_or_else(|| WfdbError::Header("empty header".into()))?;
    let rec: Vec<&str> = record_line.split_whitespace().collect();
    if rec.len() < 2 {
        return Err(WfdbError::Header(format!(
            "record line too short: {record_line:?}"
        )));
    }
    let record_name = rec[0].split('/').next().unwrap_or(rec[0]).to_string();
    let n_signals: usize = parse_num(rec[1], "signal count")?;
    let fs: f64 = rec
        .get(2)
        .map_or(Ok(250.0), |f| parse_num(f, "sampling frequency"))?;
    if fs <= 0.0 {
        return Err(WfdbError::Header(format!(
            "non-positive sampling frequency {fs}"
        )));
    }
    let n_samples: usize = rec.get(3).map_or(Ok(0), |f| parse_num(f, "sample count"))?;

    let mut signals = Vec::with_capacity(n_signals);
    for (i, line) in lines.take(n_signals).enumerate() {
        let tok: Vec<&str> = line.split_whitespace().collect();
        if tok.len() < 2 {
            return Err(WfdbError::Header(format!(
                "signal line too short: {line:?}"
            )));
        }
        let format: u32 = parse_num(tok[1], "format")?;
        let (gain, baseline, units) = match tok.get(2) {
            Some(g) => parse_gain(g)?,
            None => (200.0, None, "mV".to_string()),
        };
        let adc_zero: i32 = tok.get(4).map_or(Ok(0), |f| parse_num(f, "ADC zero"))?;
        let init_value = tok
            .get(5)
            .map(|f| parse_num(f, "initial value"))
            .transpose()?;
        let checksum = tok.get(6).map(|f| parse_num(f, "checksum")).transpose()?;
        let name = if tok.len() > 8 {
            tok[8..].join(" ")
        } else {
            format!("sig{i}")
        };
        signals.push(SignalSpec {
            file: tok[0].to_string(),
            format,
            gain,
            baseline: baseline.unwrap_or(adc_zero),
            units,
            init_value,
            checksum,
            name,
        });
    }
    if signals.len() != n_signals {
        return Err(WfdbError::Header(format!(
            "expected {n_signals} signal lines, found {}",
            signals.len()
        )));
    }
    Ok(Header {
        record_name,
        fs,
        n_samples,
        signals,
    })
}

/// WFDB checksum: 16-bit two's-complement sum of all samples of a signal.
pub fn checksum(raw: impl IntoIterator<Item = i16>) -> i16 {
    raw.into_iter().fold(0i16, |acc, v| acc.wrapping_add(v))
}

/// Decode interleaved little-endian 16-bit samples (WFDB format 16) for all
/// signals in `header`, which must all live in the same file.
pub fn decode_format16(header: &Header, bytes: &[u8]) -> Result<Vec<Lead>, WfdbError> {
    let nsig = header.signals.len();
    if nsig == 0 {
        return Err(WfdbError::Data("record has no signals".into()));
    }
    if let Some(s) = header.signals.iter().find(|s| s.format != 16) {
        return Err(WfdbError::Data(format!(
            "signal {} uses format {}, only format 16 is supported",
            s.name, s.format
        )));
    }
    let frame_bytes = 2 * nsig;
    if !bytes.len().is_multiple_of(frame_bytes) {
        return Err(WfdbError::Data(format!(
            "file length {} is not a multiple of the frame size {frame_bytes}",
            bytes.len()
        )));
    }
    let n_frames = bytes.len() / frame_bytes;
    if header.n_samples != 0 && n_frames != header.n_samples {
        return Err(WfdbError::Data(format!(
            "header says {} samples per signal, file holds {n_frames}",
            header.n_samples
        )));
    }

    let mut raw: Vec<Vec<i16>> = vec![Vec::with_capacity(n_frames); nsig];
    for frame in bytes.chunks_exact(frame_bytes) {
        for (sig, pair) in frame.chunks_exact(2).enumerate() {
            raw[sig].push(i16::from_le_bytes([pair[0], pair[1]]));
        }
    }

    header
        .signals
        .iter()
        .zip(raw)
        .map(|(spec, values)| {
            if let (Some(expected), Some(&first)) = (spec.init_value, values.first())
                && i32::from(first) != expected
            {
                return Err(WfdbError::Data(format!(
                    "{}: first sample {first} != header initial value {expected}",
                    spec.name
                )));
            }
            if let Some(expected) = spec.checksum {
                let actual = checksum(values.iter().copied());
                if i32::from(actual) != expected as i16 as i32 {
                    return Err(WfdbError::Data(format!(
                        "{}: checksum {actual} != header checksum {expected}",
                        spec.name
                    )));
                }
            }
            let samples = values
                .iter()
                .map(|&v| {
                    if v == INVALID_SAMPLE {
                        f32::NAN
                    } else {
                        ((f64::from(v) - f64::from(spec.baseline)) / spec.gain) as f32
                    }
                })
                .collect();
            Ok(Lead {
                name: spec.name.clone(),
                units: spec.units.clone(),
                samples,
            })
        })
        .collect()
}

fn read(path: &Path) -> Result<Vec<u8>, WfdbError> {
    fs::read(path).map_err(|e| WfdbError::Io(path.to_path_buf(), e))
}

/// Load a record given the path to its `.hea` file.
pub fn load_record(hea_path: &Path) -> Result<Record, WfdbError> {
    let text = String::from_utf8(read(hea_path)?)
        .map_err(|_| WfdbError::Header(format!("{} is not UTF-8", hea_path.display())))?;
    let header = parse_header(&text)?;
    let file = &header
        .signals
        .first()
        .ok_or_else(|| WfdbError::Header("no signals".into()))?
        .file;
    if header.signals.iter().any(|s| &s.file != file) {
        return Err(WfdbError::Data(
            "signals spread over several files are not supported".into(),
        ));
    }
    let dat_path = hea_path.with_file_name(file);
    let leads = decode_format16(&header, &read(&dat_path)?)?;
    Ok(Record {
        name: header.record_name,
        fs: header.fs,
        leads,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "\
rec 3 500 4
# a comment
rec.dat 16 1000.0(0)/mV 16 0 10 0 0 I
rec.dat 16 200/mV 16 5 -3 0 0 II
rec.dat 16 1000 16 0 0 0 0 V1
";

    fn encode(frames: &[[i16; 3]]) -> Vec<u8> {
        frames
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect()
    }

    #[test]
    fn parses_record_line() {
        let h = parse_header(HEADER).unwrap();
        assert_eq!(h.record_name, "rec");
        assert_eq!(h.fs, 500.0);
        assert_eq!(h.n_samples, 4);
        assert_eq!(h.signals.len(), 3);
    }

    #[test]
    fn parses_signal_lines() {
        let h = parse_header(HEADER).unwrap();
        let s = &h.signals[0];
        assert_eq!(
            (s.file.as_str(), s.format, s.gain, s.baseline),
            ("rec.dat", 16, 1000.0, 0)
        );
        assert_eq!((s.units.as_str(), s.name.as_str()), ("mV", "I"));
        assert_eq!(s.init_value, Some(10));
        // No explicit baseline: falls back to ADC zero.
        assert_eq!((h.signals[1].gain, h.signals[1].baseline), (200.0, 5));
        assert_eq!(
            (h.signals[2].gain, h.signals[2].units.as_str()),
            (1000.0, "mV")
        );
    }

    #[test]
    fn parses_ptbxl_header() {
        let text = "00001_hr 12 500 5000\n\
                    00001_hr.dat 16 1000.0(0)/mV 16 0 -115 13047 0 I\n"
            .to_string()
            + &"00001_hr.dat 16 1000.0(0)/mV 16 0 0 0 0 X\n".repeat(11);
        let h = parse_header(&text).unwrap();
        assert_eq!(h.signals.len(), 12);
        assert_eq!(h.signals[0].checksum, Some(13047));
        assert_eq!(h.signals[0].init_value, Some(-115));
    }

    #[test]
    fn rejects_missing_signal_lines() {
        assert!(matches!(
            parse_header("rec 2 500 10\nrec.dat 16 1000 16 0 0 0 0 I\n"),
            Err(WfdbError::Header(_))
        ));
        assert!(parse_header("").is_err());
        assert!(parse_header("rec 1 0 10\nrec.dat 16\n").is_err());
    }

    #[test]
    fn leading_number_handles_wfdb_suffixes() {
        assert_eq!(leading_number("500/1000"), "500");
        assert_eq!(leading_number("16+24"), "16");
        assert_eq!(leading_number("-115"), "-115");
        assert_eq!(leading_number("1000.0(0)"), "1000.0");
    }

    #[test]
    fn decodes_interleaved_samples_to_millivolts() {
        let h = parse_header(HEADER).unwrap();
        let bytes = encode(&[
            [10, -3, 0],
            [1000, 205, -500],
            [-1000, 5, 250],
            [0, i16::MIN, 1],
        ]);
        let mut h = h;
        h.signals.iter_mut().for_each(|s| s.checksum = None);
        let leads = decode_format16(&h, &bytes).unwrap();
        assert_eq!(leads[0].samples, vec![0.01, 1.0, -1.0, 0.0]);
        assert_eq!(leads[1].samples[..3], [-0.04, 1.0, 0.0]);
        assert!(
            leads[1].samples[3].is_nan(),
            "invalid sample must decode to NaN"
        );
        assert_eq!(leads[2].samples, vec![0.0, -0.5, 0.25, 0.001]);
        assert_eq!(leads[2].name, "V1");
    }

    #[test]
    fn verifies_checksum_and_initial_value() {
        let mut h = parse_header("r 1 500 3\nr.dat 16 1000 16 0 1 0 0 I\n").unwrap();
        let bytes = encode1(&[1, 2, 3]);
        h.signals[0].checksum = Some(6);
        assert!(decode_format16(&h, &bytes).is_ok());
        h.signals[0].checksum = Some(7);
        assert!(matches!(
            decode_format16(&h, &bytes),
            Err(WfdbError::Data(_))
        ));
        h.signals[0].checksum = None;
        h.signals[0].init_value = Some(2);
        assert!(decode_format16(&h, &bytes).is_err());
    }

    fn encode1(values: &[i16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn checksum_wraps_like_wfdb() {
        assert_eq!(checksum([i16::MAX, 1]), i16::MIN);
        assert_eq!(checksum([1, 2, 3]), 6);
    }

    #[test]
    fn rejects_bad_data_length_and_format() {
        let h = parse_header("r 1 500 3\nr.dat 16 1000 16 0\n").unwrap();
        assert!(
            decode_format16(&h, &encode1(&[1, 2])).is_err(),
            "sample count mismatch"
        );
        assert!(decode_format16(&h, &[0u8; 5]).is_err(), "odd byte count");
        let h = parse_header("r 1 500 3\nr.dat 212 1000 16 0\n").unwrap();
        assert!(
            decode_format16(&h, &[0u8; 6]).is_err(),
            "format 212 unsupported"
        );
    }

    #[test]
    fn loads_record_from_disk() {
        let dir = std::env::temp_dir().join(format!("ecgdisp-wfdb-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("t.hea"),
            "t 2 100 2\nt.dat 16 1000(0)/mV 16 0 5 7 0 I\nt.dat 16 1000(0)/mV 16 0 -5 -7 0 AVR\n",
        )
        .unwrap();
        fs::write(dir.join("t.dat"), encode1(&[5, -5, 2, -2])).unwrap();
        let rec = load_record(&dir.join("t.hea")).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(rec.name, "t");
        assert_eq!(rec.n_samples(), 2);
        assert_eq!(rec.duration_s(), 0.02);
        assert_eq!(rec.lead("aVR").unwrap().samples, vec![-0.005, -0.002]);
        assert!(rec.lead("V1").is_none());
    }

    #[test]
    fn load_reports_missing_file() {
        let err = load_record(Path::new("/nonexistent/x.hea")).unwrap_err();
        assert!(matches!(err, WfdbError::Io(..)));
        assert!(err.to_string().contains("x.hea"));
    }
}
