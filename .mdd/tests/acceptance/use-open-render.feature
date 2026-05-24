# @id(AT-USE-OPEN-RENDER)
# @ref(USE-OPEN-RENDER)
# Acceptance scaffold: a contextual "Open render" button on the view-tab row
# launches the selected mockup's React render in the browser, starting the
# Vite dev server (mockups/, port 4317) only if it is not already running.
Feature: Open a mockup's React render from the viewer

  Background:
    Given the viewer is open on this project

  Scenario: The button is enabled for a mockup that has a React render
    Given the developer selects a mockup .puml whose slug has a render
    Then the view-tab row shows an enabled "Open render" button

  Scenario: The button is disabled when the selection has no render
    Given the developer selects a model file with no React render
    Then the "Open render" button is disabled

  Scenario: Clicking starts the dev server if needed and opens the browser
    Given the developer selects a mockup whose slug has a render
    And the mockups dev server is not already serving on port 4317
    When the developer clicks "Open render"
    Then "npm run dev" is started in the mockups/ directory
    And the OS browser opens at /mockup/<slug>

  Scenario: An already-running server is reused
    Given the mockups dev server is already serving on port 4317
    When the developer clicks "Open render"
    Then no second dev server is started
    And the OS browser opens at /mockup/<slug>
