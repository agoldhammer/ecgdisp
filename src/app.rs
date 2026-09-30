//! The egui window: stacked lead traces over a timed grid, with a hover cursor
//! that reads out every trace's value, and previous/next record navigation.

use std::path::Path;

use egui::{Align2, Color32, FontId, Key, Pos2, Rect, Sense, Stroke, pos2};

use crate::layout::{self, Range, TickLevel, TimeAxis};
use crate::leads::{self, LeadName};
use crate::nav::RecordList;
use crate::wfdb::{self, Lead, Record};

const LABEL_W: f32 = 56.0;
const VALUE_W: f32 = 120.0;
const HEADER_H: f32 = 34.0;
const AXIS_H: f32 = 40.0;

const PAPER: Color32 = Color32::from_rgb(255, 250, 247);
const GRID_MINOR: Color32 = Color32::from_rgb(246, 214, 214);
const GRID_HALF: Color32 = Color32::from_rgb(226, 150, 150);
const GRID_SECOND: Color32 = Color32::from_rgb(190, 80, 80);
const STRIP_EDGE: Color32 = Color32::from_rgb(170, 170, 170);
const BASELINE: Color32 = Color32::from_rgb(200, 200, 215);
const TRACE: Color32 = Color32::from_rgb(20, 20, 60);
const TEXT: Color32 = Color32::from_rgb(40, 40, 40);
const CURSOR: Color32 = Color32::from_rgb(0, 110, 200);
const ERROR: Color32 = Color32::from_rgb(200, 30, 30);

fn grid_stroke(level: TickLevel) -> (Stroke, f32) {
    // (grid line stroke, tick length below the plot)
    match level {
        TickLevel::Minor => (Stroke::new(0.6, GRID_MINOR), 4.0),
        TickLevel::Half => (Stroke::new(1.2, GRID_HALF), 8.0),
        TickLevel::Second => (Stroke::new(1.8, GRID_SECOND), 12.0),
    }
}

/// One record's traces, ready to paint.
pub struct Chart {
    /// Shown at the top of the chart, e.g. `Record 1  (00001_hr, 500 Hz)`.
    title: String,
    fs: f64,
    duration_s: f64,
    leads: Vec<Lead>,
    ranges: Vec<Range>,
    /// Sorted by level so bolder lines are drawn on top of lighter ones.
    ticks: Vec<(f64, TickLevel)>,
}

impl Chart {
    /// `leads` are the traces to show, top to bottom.
    pub fn new(record: &Record, leads: Vec<Lead>) -> Self {
        let duration_s = record.duration_s();
        let ranges = layout::strip_ranges(
            &leads
                .iter()
                .map(|l| l.samples.as_slice())
                .collect::<Vec<_>>(),
        );
        let mut ticks = layout::time_ticks(duration_s);
        ticks.sort_by_key(|&(_, level)| level);
        Self {
            title: record_title(record),
            fs: record.fs,
            duration_s,
            leads,
            ranges,
            ticks,
        }
    }

    fn sample_to_pos(&self, axis: &TimeAxis, range: &Range, strip: Rect, i: usize, v: f32) -> Pos2 {
        pos2(
            axis.time_to_x(i as f64 / self.fs),
            range.value_to_y(v, strip.top(), strip.bottom()),
        )
    }

    /// `notice` is drawn in the middle of the header, in `notice_color`.
    fn paint(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        hover: Option<Pos2>,
        notice: &str,
        notice_color: Color32,
    ) {
        painter.rect_filled(rect, 0.0, PAPER);
        let plot = Rect::from_min_max(
            pos2(rect.left() + LABEL_W, rect.top() + HEADER_H),
            pos2(rect.right() - VALUE_W, rect.bottom() - AXIS_H),
        );
        if plot.width() < 10.0 || plot.height() < 10.0 || self.leads.is_empty() {
            return;
        }
        let axis = TimeAxis {
            left: plot.left(),
            right: plot.right(),
            duration_s: self.duration_s,
        };
        let small = FontId::proportional(12.0);

        // Time grid and axis ticks.
        for &(t, level) in &self.ticks {
            let x = axis.time_to_x(t);
            let (stroke, tick_len) = grid_stroke(level);
            painter.vline(x, plot.y_range(), stroke);
            let tick_stroke = Stroke::new(stroke.width, TEXT);
            painter.vline(x, plot.bottom()..=plot.bottom() + tick_len, tick_stroke);
            if level != TickLevel::Minor {
                painter.text(
                    pos2(x, plot.bottom() + 14.0),
                    Align2::CENTER_TOP,
                    format!("{t:.1}"),
                    small.clone(),
                    TEXT,
                );
            }
        }
        painter.text(
            pos2(plot.right(), rect.bottom() - 2.0),
            Align2::RIGHT_BOTTOM,
            "time (s)",
            small.clone(),
            TEXT,
        );

        // Traces, one horizontal strip per lead.
        let strip_h = plot.height() / self.leads.len() as f32;
        let strips: Vec<Rect> = (0..self.leads.len())
            .map(|i| {
                let top = plot.top() + i as f32 * strip_h;
                Rect::from_min_max(pos2(plot.left(), top), pos2(plot.right(), top + strip_h))
            })
            .collect();
        for ((lead, range), strip) in self.leads.iter().zip(&self.ranges).zip(&strips) {
            painter.hline(plot.x_range(), strip.bottom(), Stroke::new(1.0, STRIP_EDGE));
            if (range.lo..=range.hi).contains(&0.0) {
                let y0 = range.value_to_y(0.0, strip.top(), strip.bottom());
                painter.hline(plot.x_range(), y0, Stroke::new(1.0, BASELINE));
            }
            painter.text(
                pos2(rect.left() + 8.0, strip.center().y),
                Align2::LEFT_CENTER,
                &lead.name,
                FontId::proportional(16.0),
                TEXT,
            );
            let clipped = painter.with_clip_rect(*strip);
            for run in layout::finite_runs(&lead.samples) {
                let points = run
                    .map(|i| self.sample_to_pos(&axis, range, *strip, i, lead.samples[i]))
                    .collect();
                clipped.line(points, Stroke::new(1.2, TRACE));
            }
        }
        painter.hline(plot.x_range(), plot.top(), Stroke::new(1.0, STRIP_EDGE));

        // Title and cursor read-out.
        let header_y = rect.top() + HEADER_H / 2.0;
        painter.text(
            pos2(plot.left(), header_y),
            Align2::LEFT_CENTER,
            &self.title,
            FontId::proportional(18.0),
            TEXT,
        );
        painter.text(
            pos2(plot.center().x, header_y),
            Align2::CENTER_CENTER,
            notice,
            FontId::proportional(14.0),
            notice_color,
        );
        let n = self.leads[0].samples.len();
        let Some(idx) = hover
            .filter(|p| plot.y_range().contains(p.y))
            .and_then(|p| axis.x_to_time(p.x))
            .and_then(|t| layout::sample_index(t, self.fs, n))
        else {
            painter.text(
                pos2(plot.right(), header_y),
                Align2::RIGHT_CENTER,
                "hover to read values",
                FontId::proportional(14.0),
                TEXT,
            );
            return;
        };
        let t = idx as f64 / self.fs;
        let x = axis.time_to_x(t);
        painter.vline(x, plot.y_range(), Stroke::new(1.0, CURSOR));
        painter.text(
            pos2(plot.right(), header_y),
            Align2::RIGHT_CENTER,
            format!("t = {t:.3} s  (sample {idx})"),
            FontId::proportional(14.0),
            CURSOR,
        );
        for ((lead, range), strip) in self.leads.iter().zip(&self.ranges).zip(&strips) {
            let v = lead.samples.get(idx).copied().unwrap_or(f32::NAN);
            if v.is_finite() {
                let p = self.sample_to_pos(&axis, range, *strip, idx, v);
                if strip.contains(p) {
                    painter.circle_filled(p, 3.5, CURSOR);
                }
            }
            painter.text(
                pos2(plot.right() + 10.0, strip.center().y),
                Align2::LEFT_CENTER,
                layout::format_value(v, &lead.units),
                FontId::monospace(14.0),
                CURSOR,
            );
        }
    }
}

fn record_title(record: &Record) -> String {
    match record.number() {
        Some(n) => format!("Record {n}  ({}, {} Hz)", record.name, record.fs),
        None => format!("Record {}  ({} Hz)", record.name, record.fs),
    }
}

/// Load a record and pick out the requested leads.
pub fn load_chart(hea: &Path, wanted: &[LeadName]) -> Result<Chart, String> {
    let record = wfdb::load_record(hea).map_err(|e| e.to_string())?;
    let traces = leads::select(&record, wanted)?;
    Ok(Chart::new(&record, traces))
}

pub fn window_title(chart: &Chart) -> String {
    format!("ecgdisp — {}", chart.title)
}

pub struct EcgApp {
    chart: Chart,
    records: RecordList,
    wanted: Vec<LeadName>,
    /// Why the last navigation skipped records, if it did.
    error: Option<String>,
}

impl EcgApp {
    /// `chart` must be the record at `records.current()`.
    pub fn new(chart: Chart, records: RecordList, wanted: Vec<LeadName>) -> Self {
        Self {
            chart,
            records,
            wanted,
            error: None,
        }
    }

    /// Move `step` (±1) records, skipping any that fail to load.
    /// Returns true if the displayed record changed.
    fn navigate(&mut self, step: isize) -> bool {
        let mut errors = Vec::new();
        let mut loaded = None;
        for (index, path) in self.records.candidates(step) {
            match load_chart(path, &self.wanted) {
                Ok(chart) => {
                    loaded = Some((index, chart));
                    break;
                }
                Err(e) => errors.push(e),
            }
        }
        self.error = match errors.len() {
            0 => None,
            1 => Some(format!("skipped: {}", errors[0])),
            n => Some(format!("skipped {n} records; last: {}", errors[n - 1])),
        };
        match loaded {
            Some((index, chart)) => {
                self.records.set_current(index);
                self.chart = chart;
                true
            }
            None => false,
        }
    }

    fn notice(&self) -> (String, Color32) {
        if let Some(e) = &self.error {
            return (e.clone(), ERROR);
        }
        let (pos, total) = self.records.position();
        (
            format!("{pos} of {total} in folder  ·  Left / Right or PgUp / PgDn: previous / next"),
            TEXT,
        )
    }
}

impl eframe::App for EcgApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let step = ui.input(|i| {
            if i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::PageUp) {
                -1
            } else if i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::PageDown) {
                1
            } else {
                0
            }
        });
        if step != 0 && self.navigate(step) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(window_title(&self.chart)));
        }
        let (notice, color) = self.notice();
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::hover());
        self.chart.paint(
            &painter,
            response.rect,
            response.hover_pos(),
            &notice,
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a one-lead, two-sample record `name` into `dir`.
    fn write_record(dir: &Path, name: &str, with_data: bool) {
        std::fs::write(
            dir.join(format!("{name}.hea")),
            format!("{name} 1 500 2\n{name}.dat 16 1000(0)/mV 16 0 0 0 0 II\n"),
        )
        .unwrap();
        if with_data {
            std::fs::write(dir.join(format!("{name}.dat")), [0u8; 4]).unwrap();
        }
    }

    #[test]
    fn navigation_steps_between_records_and_skips_broken_ones() {
        let dir = std::env::temp_dir().join(format!("ecgdisp-app-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_record(&dir, "00001_hr", true);
        write_record(&dir, "00002_hr", false); // .dat missing
        write_record(&dir, "00003_hr", true);
        let wanted = vec![LeadName::II];
        let start = dir.join("00001_hr.hea");
        let chart = load_chart(&start, &wanted).unwrap();
        let mut app = EcgApp::new(chart, RecordList::scan(&start), wanted);

        let back_at_start = app.navigate(-1);
        let forward = app.navigate(1);
        let (fwd_title, fwd_pos, fwd_err) = (
            app.chart.title.clone(),
            app.records.position(),
            app.error.clone(),
        );
        let past_end = app.navigate(1);
        let end_err = app.error.clone();
        let back = app.navigate(-1);
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(!back_at_start, "nothing before the first record");
        assert!(forward);
        assert!(fwd_title.starts_with("Record 3 "), "{fwd_title}");
        assert_eq!(fwd_pos, (3, 3));
        assert!(fwd_err.unwrap().contains("00002_hr.dat"));
        assert!(!past_end);
        assert_eq!(end_err, None);
        assert!(back);
        assert!(app.chart.title.starts_with("Record 1 "));
    }

    #[test]
    fn title_shows_record_number() {
        let rec = |name: &str| Record {
            name: name.into(),
            fs: 500.0,
            leads: vec![],
        };
        assert_eq!(
            record_title(&rec("00042_hr")),
            "Record 42  (00042_hr, 500 Hz)"
        );
        assert_eq!(record_title(&rec("test")), "Record test  (500 Hz)");
    }
}
