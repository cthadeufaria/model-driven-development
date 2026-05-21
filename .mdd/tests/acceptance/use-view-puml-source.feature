# @id(AT-USE-VIEW-PUML-SOURCE)
# @ref(USE-VIEW-PUML-SOURCE)
# Acceptance scaffold: a "Source" toggle next to the "Diagram" (and "Diff")
# toggle shows the selected model file's raw PlantUML text, read-only.
Feature: View a model file's PlantUML source in the app

  Background:
    Given the viewer is open on this project
    And the developer selects a model .puml file in the rail

  Scenario: A Source toggle sits next to the Diagram toggle
    Then the central panel shows a "Diagram", "Source" and "Diff" toggle row
    And the view defaults to Diagram

  Scenario: Source shows the raw PlantUML text
    When the developer clicks "Source"
    Then the canvas is replaced by the file's raw .puml text, read-only
    And @id, @ref, @desc and @sec markers are visually distinct
    And @startuml and @enduml directives are visually distinct
    And the text scrolls without invoking any renderer

  Scenario: Toggling back repaints the diagram
    Given the developer is in Source view
    When the developer clicks "Diagram"
    Then the rendered SVG is painted again with pan, zoom and fisheye

  Scenario: Source needs no rendered artifact
    Given a model file whose SVG has not been rendered
    When the developer clicks "Source"
    Then the raw .puml text is still shown
