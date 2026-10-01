//! A title bar, border and resize edges drawn by the app itself, for a window
//! opened without a platform frame (under WSLg; see `main.rs`).

use egui::{
    Align2, Color32, CursorIcon, FontId, Id, PointerButton, Rect, ResizeDirection, Sense, Stroke,
    Ui, ViewportCommand, pos2, vec2,
};

use crate::layout::{self, Edge};

pub const HEIGHT: f32 = 32.0;
const BUTTON_W: f32 = 46.0;
/// How close to a side the pointer must be to resize, and how far along
/// the side a corner reaches.
const RESIZE_MARGIN: f32 = 6.0;
const RESIZE_CORNER: f32 = 18.0;

const BAR: Color32 = Color32::from_rgb(236, 232, 230);
const BAR_HOVER: Color32 = Color32::from_rgb(218, 212, 209);
const CLOSE_HOVER: Color32 = Color32::from_rgb(196, 43, 28);
const BORDER: Color32 = Color32::from_rgb(150, 146, 144);
const TEXT: Color32 = Color32::from_rgb(40, 40, 40);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Button {
    Minimize,
    Maximize,
    Close,
}

fn is_maximized(ui: &Ui) -> bool {
    ui.input(|i| i.viewport().maximized.unwrap_or(false))
}

fn direction(edge: Edge) -> (ResizeDirection, CursorIcon) {
    match edge {
        Edge::North => (ResizeDirection::North, CursorIcon::ResizeNorth),
        Edge::South => (ResizeDirection::South, CursorIcon::ResizeSouth),
        Edge::East => (ResizeDirection::East, CursorIcon::ResizeEast),
        Edge::West => (ResizeDirection::West, CursorIcon::ResizeWest),
        Edge::NorthEast => (ResizeDirection::NorthEast, CursorIcon::ResizeNorthEast),
        Edge::NorthWest => (ResizeDirection::NorthWest, CursorIcon::ResizeNorthWest),
        Edge::SouthEast => (ResizeDirection::SouthEast, CursorIcon::ResizeSouthEast),
        Edge::SouthWest => (ResizeDirection::SouthWest, CursorIcon::ResizeSouthWest),
    }
}

/// Show a resize cursor near the sides of `window`, and start resizing on a
/// press there. Returns true while the pointer is over a resize edge.
pub fn resize_edges(ui: &Ui, window: Rect) -> bool {
    if is_maximized(ui) {
        return false;
    }
    let Some(pos) = ui.input(|i| i.pointer.hover_pos()) else {
        return false;
    };
    let Some(edge) = layout::resize_edge(
        pos.x,
        pos.y,
        window.left(),
        window.top(),
        window.right(),
        window.bottom(),
        RESIZE_MARGIN,
        RESIZE_CORNER,
    ) else {
        return false;
    };
    let (dir, icon) = direction(edge);
    ui.ctx().set_cursor_icon(icon);
    if ui.input(|i| i.pointer.primary_pressed()) {
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::BeginResize(dir));
    }
    true
}

/// Draw the title bar in `bar`. Dragging it moves the window (unless the
/// press is on a resize edge), double-clicking maximizes or restores, and
/// the buttons at the right minimize, maximize/restore and close.
pub fn title_bar(ui: &mut Ui, bar: Rect, title: &str, on_resize_edge: bool) {
    let maximized = is_maximized(ui);
    let painter = ui.painter_at(bar);
    painter.rect_filled(bar, 0.0, BAR);
    painter.hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, BORDER));
    painter.text(
        bar.center(),
        Align2::CENTER_CENTER,
        title,
        FontId::proportional(14.0),
        TEXT,
    );

    let drag = ui.interact(bar, Id::new("title_bar"), Sense::click_and_drag());
    if drag.double_clicked() {
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    } else if drag.drag_started_by(PointerButton::Primary) && !on_resize_edge {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }

    // Buttons from the right: close, maximize/restore, minimize. Registered
    // after the drag area so they take the clicks over it.
    let size = vec2(BUTTON_W, bar.height() - 1.0);
    for (i, button) in [Button::Close, Button::Maximize, Button::Minimize]
        .into_iter()
        .enumerate()
    {
        let right = bar.right() - i as f32 * BUTTON_W;
        let rect = Rect::from_min_size(pos2(right - BUTTON_W, bar.top()), size);
        let response = ui.interact(rect, Id::new(("title_button", button)), Sense::click());
        let hovered = response.hovered() && !on_resize_edge;
        let ink = if hovered && button == Button::Close {
            painter.rect_filled(rect, 0.0, CLOSE_HOVER);
            Color32::WHITE
        } else {
            if hovered {
                painter.rect_filled(rect, 0.0, BAR_HOVER);
            }
            TEXT
        };
        paint_glyph(
            &painter,
            rect.center(),
            button,
            maximized,
            Stroke::new(1.0, ink),
        );
        if response.clicked() && !on_resize_edge {
            let cmd = match button {
                Button::Minimize => ViewportCommand::Minimized(true),
                Button::Maximize => ViewportCommand::Maximized(!maximized),
                Button::Close => ViewportCommand::Close,
            };
            ui.ctx().send_viewport_cmd(cmd);
        }
    }
}

fn paint_glyph(
    painter: &egui::Painter,
    c: egui::Pos2,
    button: Button,
    maximized: bool,
    stroke: Stroke,
) {
    const R: f32 = 5.0;
    match button {
        Button::Minimize => {
            painter.hline(c.x - R..=c.x + R, c.y, stroke);
        }
        Button::Maximize if maximized => {
            // Restore: two overlapping squares.
            let back = Rect::from_min_size(pos2(c.x - R + 2.0, c.y - R), vec2(8.0, 8.0));
            let front = Rect::from_min_size(pos2(c.x - R, c.y - R + 2.0), vec2(8.0, 8.0));
            painter.hline(back.left()..=back.right(), back.top(), stroke);
            painter.vline(back.right(), back.top()..=back.bottom(), stroke);
            painter.rect_stroke(front, 0.0, stroke, egui::StrokeKind::Middle);
        }
        Button::Maximize => {
            painter.rect_stroke(
                Rect::from_center_size(c, vec2(2.0 * R, 2.0 * R)),
                0.0,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Button::Close => {
            painter.line_segment([pos2(c.x - R, c.y - R), pos2(c.x + R, c.y + R)], stroke);
            painter.line_segment([pos2(c.x - R, c.y + R), pos2(c.x + R, c.y - R)], stroke);
        }
    }
}

/// Outline the window so it stands apart from the desktop when not maximized.
pub fn border(ui: &Ui, window: Rect) {
    if !is_maximized(ui) {
        ui.painter().rect_stroke(
            window.shrink(0.5),
            0.0,
            Stroke::new(1.0, BORDER),
            egui::StrokeKind::Inside,
        );
    }
}
