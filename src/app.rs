//! The egui window: stacked lead traces over a timed grid, with a hover cursor
//! that reads out every trace's value.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, pos2};

use crate::layout::{self, Range, TickLevel, TimeAxis};
use crate::wfdb::{Lead, Record};

const LABEL_W: f32 = 56.0;
const VALUE_W: f32 = 120.0;
const HEADER_H: f32 = 28.0;
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

fn grid_stroke(level: TickLevel) -> (Stroke, f32) {
    // (grid line stroke, tick length below the plot)
    match level {
        TickLevel::Minor => (Stroke::new(0.6, GRID_MINOR), 4.0),
        TickLevel::Half => (Stroke::new(1.2, GRID_HALF), 8.0),
        TickLevel::Second => (Stroke::new(1.8, GRID_SECOND), 12.0),
    }
}

pub struct EcgApp {
    record_name: String,
    fs: f64,
    duration_s: f64,
    leads: Vec<Lead>,
    ranges: Vec<Range>,
    /// Sorted by level so bolder lines are drawn on top of lighter ones.
    ticks: Vec<(f64, TickLevel)>,
}

impl EcgApp {
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
            record_name: record.name.clone(),
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

    fn paint(&self, painter: &egui::Painter, rect: Rect, hover: Option<Pos2>) {
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

        // Cursor with per-trace read-out.
        let n = self.leads[0].samples.len();
        let Some(idx) = hover
            .filter(|p| plot.y_range().contains(p.y))
            .and_then(|p| axis.x_to_time(p.x))
            .and_then(|t| layout::sample_index(t, self.fs, n))
        else {
            painter.text(
                pos2(plot.left(), rect.top() + HEADER_H / 2.0),
                Align2::LEFT_CENTER,
                format!(
                    "{}   {} Hz   (hover to read values)",
                    self.record_name, self.fs
                ),
                FontId::proportional(14.0),
                TEXT,
            );
            return;
        };
        let t = idx as f64 / self.fs;
        let x = axis.time_to_x(t);
        painter.vline(x, plot.y_range(), Stroke::new(1.0, CURSOR));
        painter.text(
            pos2(plot.left(), rect.top() + HEADER_H / 2.0),
            Align2::LEFT_CENTER,
            format!(
                "{}   {} Hz   t = {t:.3} s  (sample {idx})",
                self.record_name, self.fs
            ),
            FontId::proportional(14.0),
            TEXT,
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

impl eframe::App for EcgApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::hover());
        self.paint(&painter, response.rect, response.hover_pos());
    }
}
