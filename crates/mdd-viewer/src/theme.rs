//! Tier-1 theming for `mdd view` (USE-VIEWER-THEMED, SEQ-APPLY-THEME,
//! CMP-VIEWER-THEME).
//!
//! Pure chrome boundary: this module knows about [`egui::Context`] and
//! a handful of font/theme crates, but it never touches the SVG render
//! pipeline (CMP-VIEWER-RENDER-PIPELINE) or the model/cycle registries.
//! Removing it must leave every rendered diagram pixel-identical; only
//! how `egui` paints panels, buttons, and text changes.

use eframe::egui::{self, Context, FontData, FontDefinitions, FontFamily};

/// Bundled Inter Regular — used as the default `Proportional` face.
const INTER_REGULAR: &[u8] =
    include_bytes!("../assets/fonts/Inter-Regular.ttf");
/// Bundled Inter Medium — registered as a fallback in `Proportional`
/// for heavier weights egui may select.
const INTER_MEDIUM: &[u8] =
    include_bytes!("../assets/fonts/Inter-Medium.ttf");
/// Bundled JetBrains Mono Regular — used as the default `Monospace`
/// face for code-like panes (diff lists, OCL source, IDs).
const JETBRAINS_MONO_REGULAR: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

/// Install the bundled Inter + JetBrains Mono families on `ctx` as the
/// default Proportional and Monospace faces, keeping egui's built-in
/// faces as fallbacks. Call once from `MddViewer::new` after the
/// `eframe` context is available.
///
/// Realizes the construction half of SEQ-APPLY-THEME.
pub fn register_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "Inter-Regular".to_owned(),
        FontData::from_static(INTER_REGULAR).into(),
    );
    fonts.font_data.insert(
        "Inter-Medium".to_owned(),
        FontData::from_static(INTER_MEDIUM).into(),
    );
    fonts.font_data.insert(
        "JetBrainsMono-Regular".to_owned(),
        FontData::from_static(JETBRAINS_MONO_REGULAR).into(),
    );

    // Insert at the front of each family so Inter / JetBrains Mono are
    // tried before egui's default faces, but the defaults remain as
    // fallback for glyphs Inter/JBMono don't cover.
    if let Some(proportional) = fonts.families.get_mut(&FontFamily::Proportional) {
        proportional.insert(0, "Inter-Medium".to_owned());
        proportional.insert(0, "Inter-Regular".to_owned());
    }
    if let Some(monospace) = fonts.families.get_mut(&FontFamily::Monospace) {
        monospace.insert(0, "JetBrainsMono-Regular".to_owned());
    }

    ctx.set_fonts(fonts);
}

/// Apply the catppuccin palette and the Tier-1 spacing/rounding
/// overrides for one frame. Call from `MddViewer::update` before any
/// panel is drawn; the call is idempotent and cheap (the catppuccin
/// theme is a plain `Visuals` assignment and `Style` mutations are
/// in-place), so re-applying every frame lets the chrome pick up live
/// OS appearance changes (System Settings → Appearance) without a
/// restart.
///
/// Realizes the per-frame half of SEQ-APPLY-THEME.
pub fn apply_theme(ctx: &Context) {
    let mode = dark_light::detect().unwrap_or(dark_light::Mode::Unspecified);
    match mode {
        dark_light::Mode::Light => {
            catppuccin_egui::set_theme(ctx, catppuccin_egui::LATTE);
        }
        // Dark + Unspecified both fall through to Mocha — the
        // Catppuccin demo image and the viewer's diagram canvases
        // (which paint white-background SVGs) both read better against
        // a dark chrome by default.
        dark_light::Mode::Dark | dark_light::Mode::Unspecified => {
            catppuccin_egui::set_theme(ctx, catppuccin_egui::MOCHA);
        }
    }

    ctx.style_mut(|style| {
        // Slightly more breathing room than egui's default (4, 4) —
        // labels and chevrons stop colliding with rail rows.
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);

        // Soften the boxy default corners on every widget variant.
        let rounding: egui::Rounding = 6.0.into();
        style.visuals.window_rounding = rounding;
        style.visuals.menu_rounding = rounding;
        style.visuals.widgets.noninteractive.rounding = rounding;
        style.visuals.widgets.inactive.rounding = rounding;
        style.visuals.widgets.hovered.rounding = rounding;
        style.visuals.widgets.active.rounding = rounding;
        style.visuals.widgets.open.rounding = rounding;
    });
}
