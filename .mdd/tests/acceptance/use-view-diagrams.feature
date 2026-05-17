# @id(AT-USE-VIEW-DIAGRAMS)
# @ref(USE-VIEW-DIAGRAMS)
# Acceptance scaffold for the objective: diagram text is legible in the
# in-app canvas, not only in an external SVG viewer.
Feature: View rendered diagram with legible text

  Background:
    Given a project with at least one rendered diagram under .mdd/rendered/
    And the diagram source contains text labels

  Scenario: Diagram text is rasterized in the viewer canvas
    Given the mdd viewer is open
    When the developer selects the rendered diagram in the left rail
    Then the canvas shows the diagram shapes
    And the canvas shows the diagram text labels
    And no glyphs are dropped because of an empty font database

  Scenario: Font database is populated before the first parse
    Given the host has system fonts installed
    When the viewer loads any rendered SVG
    Then the usvg font database reports at least one font
    And usvg::Options carries a non-empty default font family

  Scenario: Empty model set still shows the placeholder
    Given no models are mapped or rendered
    When the viewer opens
    Then the canvas shows "No rendered output — run the render pipeline first"
