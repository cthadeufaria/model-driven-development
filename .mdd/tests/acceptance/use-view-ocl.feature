# @id(AT-USE-VIEW-OCL)
# @ref(USE-VIEW-OCL)
# Acceptance scaffold: selecting an .ocl constraint file shows a
# Source/Diagram toggle on the canvas (Source = raw OCL text, Diagram =
# synthesized constraints diagram).
Feature: View an OCL constraint file in the app

  Background:
    Given the viewer is open on this project

  Scenario: Source sub-mode shows raw OCL text
    Given the developer selects .mdd/constraints/cycle-tracking.ocl in the rail
    When the OCL view is in Source sub-mode
    Then the canvas shows the raw OCL text, read-only
    And @id, context and inv lines are visually distinct

  Scenario: Diagram sub-mode shows the synthesized constraints diagram
    Given the developer toggles the OCL view to Diagram
    And the constraints diagram has been rendered
    Then the canvas paints .mdd/rendered/constraints/cycle-tracking.svg
    And pan and zoom work as on the normal diagram canvas

  Scenario: Diagram sub-mode without a rendered SVG
    Given the OCL view is in Diagram sub-mode
    And the constraints diagram has not been rendered
    Then a placeholder tells the developer to run the render pipeline

  Scenario: Default sub-mode
    Given the developer selects an .ocl file
    Then the OCL view defaults to Source
