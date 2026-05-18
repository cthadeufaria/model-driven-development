# @id(AT-USE-DIFF-DIAGRAM)
# @ref(USE-DIFF-DIAGRAM)
# Acceptance scaffold: Diff mode is bound to (selected cycle x rail-selected
# file) and has an inner Diagram/List toggle; Diagram paints the rendered
# .diff.svg, List shows that file's text buckets.
Feature: Toggle the cycle diff between a rendered diagram and the list

  Background:
    Given a closed cycle 0002 with before/ and after/ snapshots
    And the cycle's diff pumls have been rasterized to .mdd/rendered/cycles/0002/

  Scenario: Diff follows the file selected in the rail
    Given the developer selects current/domain/canvas-view.puml in the rail
    When the developer opens Diff mode for cycle 0002
    Then the diff shown is the one for canvas-view.puml in cycle 0002

  Scenario: Diagram sub-mode renders the superposed SVG
    Given Diff mode is open with the Diagram toggle active
    Then the canvas paints .mdd/rendered/cycles/0002/domain/canvas-view.diff.svg
    And additions are drawn green and removals red
    And pan and zoom work as on the normal diagram canvas

  Scenario: List sub-mode shows that one file's buckets
    Given Diff mode is open with the List toggle active
    Then only canvas-view.puml's added, removed and unchanged elements are listed

  Scenario: Selected file unchanged in the cycle
    Given the developer selects a file not touched by cycle 0002
    When the developer opens Diff mode for cycle 0002
    Then a placeholder explains the file did not change in that cycle

  Scenario: Rendered diff SVG missing
    Given the Diagram toggle is active but the diff SVG has not been rendered
    Then a placeholder tells the developer to run the render pipeline
