use crate::theme::*;
use egui::epaint::CornerRadius;
use egui::{
    self, Color32, FontFamily, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2,
};

// ── Accent Button ──────────────────────────────────────────────────

pub fn accent_button(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
    let desired_size = Vec2::new(ui.available_width().min(280.0), 36.0);
    accent_button_sized(ui, text, color, desired_size)
}

pub fn accent_button_sized(
    ui: &mut egui::Ui,
    text: &str,
    color: Color32,
    size: Vec2,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    if ui.is_rect_visible(rect) {
        let bg = if response.hovered() {
            lighten(color, 25)
        } else {
            color
        };

        ui.painter().rect_filled(rect, BUTTON_ROUNDING, bg);

        let text_color = Color32::BLACK;

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            FontId::new(12.0, FontFamily::Proportional),
            text_color,
        );
    }

    response
}

// ── Card/Panel ─────────────────────────────────────────────────────

pub fn card_frame(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(BG_CARD)
        .corner_radius(CARD_ROUNDING)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            add_contents(ui);
        });
}

// ── Status Badge ───────────────────────────────────────────────────

pub fn status_badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    let text_galley = ui.painter().layout_no_wrap(
        text.to_string(),
        FontId::new(11.0, FontFamily::Proportional),
        Color32::BLACK,
    );
    let pill_size = Vec2::new(text_galley.size().x + 14.0, 20.0);
    let (rect, _) = ui.allocate_exact_size(pill_size, Sense::hover());

    if ui.is_rect_visible(rect) {
        let bg = Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 200);
        ui.painter().rect_filled(rect, PILL_ROUNDING, bg);
        ui.painter().galley(
            Pos2::new(
                rect.left() + 7.0,
                rect.center().y - text_galley.size().y / 2.0,
            ),
            text_galley,
            Color32::TRANSPARENT,
        );
    }
}

// ── Progress Bar ───────────────────────────────────────────────────

pub fn animated_progress_bar(ui: &mut egui::Ui, progress: f32, color: Color32) {
    let desired_size = Vec2::new(ui.available_width(), 5.0);
    let (rect, _) = ui.allocate_exact_size(desired_size, Sense::hover());

    if ui.is_rect_visible(rect) {
        let rounding = CornerRadius::same(3);

        // Track
        ui.painter().rect_filled(
            rect,
            rounding,
            Color32::from_rgba_premultiplied(255, 255, 255, 10),
        );

        // Fill
        let fill_width = rect.width() * progress.clamp(0.0, 1.0);
        if fill_width > 0.5 {
            let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_width, rect.height()));
            ui.painter().rect_filled(fill_rect, rounding, color);
        }
    }
}

/// An indeterminate/pulsing progress bar for when we don't know the progress
pub fn pulsing_progress_bar(ui: &mut egui::Ui, time: f64, color: Color32) {
    let desired_size = Vec2::new(ui.available_width(), 5.0);
    let (rect, _) = ui.allocate_exact_size(desired_size, Sense::hover());

    if ui.is_rect_visible(rect) {
        let rounding = CornerRadius::same(3);

        // Track
        ui.painter().rect_filled(
            rect,
            rounding,
            Color32::from_rgba_premultiplied(255, 255, 255, 10),
        );

        // Pulsing segment
        let cycle = (time * 0.8) % 1.0;
        let seg_width = rect.width() * 0.3;
        let x = rect.left() + (rect.width() + seg_width) * cycle as f32 - seg_width;
        let x_min = x.max(rect.left());
        let x_max = (x + seg_width).min(rect.right());
        if x_max > x_min {
            let pulse_rect = Rect::from_min_max(
                Pos2::new(x_min, rect.top()),
                Pos2::new(x_max, rect.bottom()),
            );
            let alpha = (0.5 + 0.5 * (time * 3.0).sin() as f32).clamp(0.4, 1.0);
            let pulse_color = Color32::from_rgba_premultiplied(
                color.r(),
                color.g(),
                color.b(),
                (alpha * 180.0) as u8,
            );
            ui.painter().rect_filled(pulse_rect, rounding, pulse_color);
        }
    }
}

// ── Section Header ─────────────────────────────────────────────────

pub fn section_header(ui: &mut egui::Ui, icon: &str, text: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).size(15.0));
        ui.label(RichText::new(text).size(14.0).color(TEXT_PRIMARY).strong());
    });
}

// ── Tool Card ──────────────────────────────────────────────────────

pub fn tool_card(
    ui: &mut egui::Ui,
    name: &str,
    description: &str,
    available: bool,
    version: Option<&str>,
    color: Color32,
    selected: bool,
) -> egui::Response {
    let desired_size = Vec2::new(ui.available_width(), 56.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

    if ui.is_rect_visible(rect) {
        let bg = if selected {
            Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 200)
        } else if response.hovered() {
            BG_CARD_HOVER
        } else {
            BG_CARD
        };

        let stroke = if selected {
            Stroke::new(1.5, color)
        } else {
            Stroke::new(1.0, BORDER_SUBTLE)
        };

        ui.painter().rect_filled(rect, CARD_ROUNDING, bg);
        ui.painter()
            .rect_stroke(rect, CARD_ROUNDING, stroke, StrokeKind::Inside);

        // Dot
        let dot_center = Pos2::new(rect.left() + 16.0, rect.center().y);
        ui.painter()
            .circle_filled(dot_center, 3.5, if available { color } else { TEXT_MUTED });

        // Name
        let title_color = if selected {
            Color32::BLACK
        } else {
            TEXT_PRIMARY
        };
        ui.painter().text(
            Pos2::new(rect.left() + 30.0, rect.top() + 12.0),
            egui::Align2::LEFT_TOP,
            name,
            FontId::new(13.0, FontFamily::Proportional),
            title_color,
        );

        // Description
        let desc_color = if selected {
            Color32::from_rgb(26, 26, 26)
        } else {
            TEXT_MUTED
        };
        ui.painter().text(
            Pos2::new(rect.left() + 30.0, rect.top() + 30.0),
            egui::Align2::LEFT_TOP,
            description,
            FontId::new(11.0, FontFamily::Proportional),
            desc_color,
        );

        // Version
        let status_text = if available {
            version.unwrap_or("ok")
        } else {
            "missing"
        };
        let status_color = if available {
            if selected {
                Color32::BLACK
            } else {
                color
            }
        } else {
            WARNING
        };
        ui.painter().text(
            Pos2::new(rect.right() - 12.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            status_text,
            FontId::new(10.0, FontFamily::Monospace),
            status_color,
        );
    }

    response
}

// ── Log Output Area ────────────────────────────────────────────────

pub fn log_area(ui: &mut egui::Ui, lines: &[String], max_height: f32, stick_bottom: bool) {
    egui::Frame::NONE
        .fill(BG_INPUT)
        .corner_radius(INPUT_ROUNDING)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(max_height)
                .auto_shrink([false; 2])
                .stick_to_bottom(stick_bottom)
                .show(ui, |ui| {
                    // Keep rendering bounded for smoother UI on long sessions.
                    let start = lines.len().saturating_sub(200);
                    for line in &lines[start..] {
                        let lower = line.to_lowercase();
                        let color = if lower.contains("error") || lower.contains("fail") {
                            ERROR
                        } else if lower.contains("warn") {
                            WARNING
                        } else if lower.contains("100%")
                            || lower.contains("complete")
                            || lower.contains("done")
                        {
                            SUCCESS
                        } else {
                            TEXT_MUTED
                        };
                        ui.label(RichText::new(line).monospace().size(10.0).color(color));
                    }
                });
        });
}

// ── Code/Ticket Display ────────────────────────────────────────────

/// Display a code/ticket with a copy button. Long tickets are truncated.
pub fn code_display(ui: &mut egui::Ui, label: &str, code: &str, color: Color32) -> bool {
    let mut copied = false;

    egui::Frame::NONE
        .fill(Color32::from_rgba_premultiplied(
            color.r(),
            color.g(),
            color.b(),
            10,
        ))
        .corner_radius(CARD_ROUNDING)
        .stroke(Stroke::new(
            1.0,
            Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 30),
        ))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(11.0).color(TEXT_SECONDARY));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if accent_button_sized(ui, "📋 Copy", color, Vec2::new(70.0, 24.0)).clicked()
                    {
                        ui.ctx().copy_text(code.to_string());
                        copied = true;
                    }
                });
            });
            ui.add_space(2.0);

            // Truncate code based on available width so small windows stay usable.
            let width = ui.available_width().max(120.0);
            let budget = ((width / 8.0) as usize).clamp(20, 72);
            let display_code = truncate_middle_for_ui(code, budget);
            ui.label(
                RichText::new(&display_code)
                    .monospace()
                    .size(13.0)
                    .color(Color32::BLACK),
            );
        });

    copied
}

// ── Helpers ────────────────────────────────────────────────────────

fn lighten(color: Color32, amount: u8) -> Color32 {
    Color32::from_rgb(
        color.r().saturating_add(amount),
        color.g().saturating_add(amount),
        color.b().saturating_add(amount),
    )
}

fn truncate_middle_for_ui(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    if max_chars <= 10 {
        return value.chars().take(max_chars).collect();
    }
    let head = (max_chars.saturating_sub(1) * 2) / 3;
    let tail = max_chars.saturating_sub(head + 1);
    let start: String = value.chars().take(head).collect();
    let end: String = value
        .chars()
        .rev()
        .take(tail)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{start}…{end}")
}
