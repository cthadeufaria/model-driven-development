# AT-USE-DIAGRAMS-FIRST — acceptance for USE-DIAGRAMS-FIRST
Feature: Diagrams-first reading order
  As an agent directed by the mdd entrypoint
  I want a fresh/stale-driven reading order
  So that I never trust diagrams that have drifted from the code

  Scenario: FRESH verdict -> read diagrams then follow source_links to code
    Given "mdd context" reports FRESH
    When the agent begins work
    Then it reads the concept diagrams
    And it follows trace.yml source_links to the code before acting

  Scenario: STALE verdict -> remap the drifted area first
    Given "mdd context" reports STALE with a drift list
    When the agent begins work
    Then it runs /mdd-map on the drifted area to re-sync current/ first
    And only then reads the diagrams
