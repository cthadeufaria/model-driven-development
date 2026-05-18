# @id(AT-USE-TOGGLE-PANELS)
# @ref(USE-TOGGLE-PANELS)
# Acceptance scaffold: the left MODELS panel and right MODEL CONTEXT
# panel each collapse to a sliver and re-expand via an inner-edge handle.
Feature: Collapse and expand the side panels

  Background:
    Given the mdd viewer is open with both side panels expanded

  Scenario: Collapse the left MODELS panel
    When the developer clicks the inner-edge chevron of the left panel
    Then the left panel collapses to a thin sliver
    And the diagram canvas widens to reclaim the space

  Scenario: Re-expand a collapsed panel
    Given the left panel is collapsed to a sliver
    When the developer clicks the sliver
    Then the left panel expands back to its previous width

  Scenario: Collapse the right MODEL CONTEXT panel independently
    When the developer clicks the inner-edge chevron of the right panel
    Then the right panel collapses to a thin sliver
    And the left panel keeps its current state
