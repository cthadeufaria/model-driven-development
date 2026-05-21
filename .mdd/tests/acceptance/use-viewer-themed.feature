# @id(AT-USE-VIEWER-THEMED)
# @ref(USE-VIEWER-THEMED)
Feature: Themed viewer chrome (Tier 1 — Catppuccin + bundled fonts + OS appearance)
  In order to read and review diagrams without eye strain
  As a developer running `mdd view`
  I want the viewer chrome to follow the OS appearance and use legible bundled fonts
  So that the same single cross-platform binary looks good on macOS, Linux, and Windows
  without depending on whichever fonts the host happens to have installed.

  Background:
    Given the user runs `mdd view` in a project that has rendered SVGs under .mdd/rendered/

  Scenario: Bundled fonts are registered on startup
    When MddViewer::new constructs the viewer
    Then the egui Proportional font family resolves to bundled Inter (Regular, Medium)
    And the egui Monospace font family resolves to bundled JetBrains Mono Regular
    And no panel, button, label, or diff-list cell falls back to the egui default font

  Scenario: OS-dark hosts see the Catppuccin Mocha palette
    Given the host OS appearance is Dark
    When MddViewer::update paints a frame
    Then catppuccin_egui::set_theme(ctx, MOCHA) has been applied to ctx
    And the side panels, toolbar, tree rail, and canvas chrome use Mocha colours

  Scenario: OS-light hosts see the Catppuccin Latte palette
    Given the host OS appearance is Light
    When MddViewer::update paints a frame
    Then catppuccin_egui::set_theme(ctx, LATTE) has been applied to ctx
    And the side panels, toolbar, tree rail, and canvas chrome use Latte colours

  Scenario: OS appearance changes are picked up without restart
    Given the viewer is running with the OS appearance Dark and the Mocha palette applied
    When the user switches the OS appearance to Light
    And one more frame is painted by MddViewer::update
    Then the next frame applies catppuccin_egui::set_theme(ctx, LATTE)
    And the chrome flips to Latte without the user closing or relaunching the viewer

  Scenario: Spacing and rounding overrides are layered on top of the Catppuccin theme
    When MddViewer::update applies the theme for a frame
    Then style.spacing.item_spacing is set to (8.0, 6.0) (or larger than egui defaults)
    And widget rounding for buttons, panels, and frames is 6.0 (or larger than egui defaults)

  Scenario: Theming is chrome-only and does not touch the SVG pipeline
    When the theme is applied
    Then DOM-FONT-DATABASE (usvg fontdb populated by shared_fontdb()) is unchanged
    And no .mdd/rendered/**/*.svg is re-rasterized as a result of theming
    And no model file under .mdd/models/ is read or written by the theme module
