# @id(AT-USE-FISHEYE-FOCUS)
# @ref(USE-FISHEYE-FOCUS)
# Acceptance scaffold: the canvas applies a center-fixed fisheye layered
# on the existing scroll global zoom; the toolbar has no zoom buttons.
Feature: Focus a diagram region with the center fisheye

  Background:
    Given the mdd viewer is open with a rendered diagram on the canvas

  Scenario: The canvas centre is magnified and edges compress
    When the developer looks at the diagram canvas
    Then content near the canvas centre is magnified
    And content compresses smoothly toward the edges
    And there is no hard lens border

  Scenario: The focal point stays at the canvas centre
    When the developer moves the mouse over the canvas
    Then the magnified focal point remains at the canvas centre
    And dragging pans the diagram to bring a region into that centre

  Scenario: Zoom is scroll-only, layered under the fisheye
    When the developer scrolls the wheel over the canvas
    Then the base zoom changes at the cursor
    And the fisheye warp is still applied on top of the new base zoom

  Scenario: No toolbar zoom controls
    When the developer looks at the toolbar
    Then there are no Reset, Zoom + or Zoom - buttons
