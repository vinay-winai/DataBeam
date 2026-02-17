use egui::epaint::CornerRadius;
use egui::{Color32, FontDefinitions, FontFamily, FontId, Shadow, Stroke, Style, Vec2, Visuals};

// ── Color Palette (simple black + orange) ──────────────────────────
pub const BG_DARK: Color32 = Color32::from_rgb(8, 8, 8);
pub const BG_PANEL: Color32 = Color32::from_rgb(12, 12, 12);
pub const BG_CARD: Color32 = Color32::from_rgb(18, 18, 18);
pub const BG_CARD_HOVER: Color32 = Color32::from_rgb(28, 28, 28);
pub const BG_INPUT: Color32 = Color32::from_rgb(14, 14, 14);

// Core accent requested: #d64224
pub const ACCENT_BLUE: Color32 = Color32::from_rgb(214, 66, 36);

// High-contrast text colors
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(245, 245, 245);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(220, 220, 220);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(165, 165, 165);

pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(66, 66, 66);

// Engine colors requested:
// sendme = #d64224, croc = #57d624
pub const CROC_COLOR: Color32 = Color32::from_rgb(87, 214, 36);
pub const SENDME_COLOR: Color32 = Color32::from_rgb(214, 66, 36);

pub const SUCCESS: Color32 = Color32::from_rgb(87, 214, 36);
pub const WARNING: Color32 = Color32::from_rgb(255, 190, 95);
pub const ERROR: Color32 = Color32::from_rgb(255, 100, 100);

// ── Spacing & Sizing ───────────────────────────────────────────────
pub const CARD_ROUNDING: CornerRadius = CornerRadius::same(10);
pub const BUTTON_ROUNDING: CornerRadius = CornerRadius::same(8);
pub const INPUT_ROUNDING: CornerRadius = CornerRadius::same(6);
pub const PILL_ROUNDING: CornerRadius = CornerRadius::same(20);

pub const SPACING: Vec2 = Vec2::new(10.0, 8.0);

// ── Style Application ──────────────────────────────────────────────

pub fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.families.entry(FontFamily::Proportional).or_default();
    fonts.families.entry(FontFamily::Monospace).or_default();
    ctx.set_fonts(fonts);
}

pub fn apply_theme(ctx: &egui::Context) {
    let mut style = Style::default();
    let mut visuals = Visuals::dark();

    // Window
    visuals.window_fill = BG_PANEL;
    visuals.window_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.window_shadow = Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(80),
    };
    visuals.window_corner_radius = CARD_ROUNDING;

    // Panel
    visuals.panel_fill = BG_DARK;

    // Widgets — improved contrast
    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);
    visuals.widgets.noninteractive.corner_radius = BUTTON_ROUNDING;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);

    visuals.widgets.inactive.bg_fill = BG_CARD;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = BUTTON_ROUNDING;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);

    visuals.widgets.hovered.bg_fill = BG_CARD_HOVER;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.hovered.corner_radius = BUTTON_ROUNDING;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_BLUE);

    visuals.widgets.active.bg_fill = ACCENT_BLUE;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.active.corner_radius = BUTTON_ROUNDING;

    visuals.widgets.open.bg_fill = BG_CARD_HOVER;
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.open.corner_radius = BUTTON_ROUNDING;

    // Selection
    visuals.selection.bg_fill = Color32::from_rgba_premultiplied(214, 66, 36, 60);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT_BLUE);

    // Extreme background
    visuals.extreme_bg_color = BG_INPUT;
    visuals.faint_bg_color = BG_CARD;
    visuals.striped = true;

    style.visuals = visuals;
    style.spacing.item_spacing = SPACING;
    style.spacing.window_margin = egui::Margin::same(16);
    style.spacing.button_padding = Vec2::new(14.0, 6.0);

    // Text styles
    style.text_styles.insert(
        egui::TextStyle::Heading,
        FontId::new(20.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        FontId::new(14.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        FontId::new(13.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        FontId::new(12.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        FontId::new(13.0, FontFamily::Monospace),
    );

    ctx.set_style(style);
}
