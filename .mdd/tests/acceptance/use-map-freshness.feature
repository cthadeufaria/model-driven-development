# @id(AT-USE-MAP-FRESHNESS)
# @ref(USE-MAP-FRESHNESS)
Feature: Diagram freshness (mdd map-status)
  So that the diagrams-first reflex stays safe, an agent can ask whether the
  diagrams still reflect the code before trusting them, and is told exactly
  what drifted when they don't.

  Scenario: Fresh — nothing tracked changed since the baseline
    Given .mdd/map/manifest.yml records a source_revision
    And no symbol referenced by a source_link changed since that revision
    When the user runs `mdd map-status`
    Then it reports Fresh
    And it exits zero

  Scenario: Stale — a tracked symbol drifted
    Given .mdd/map/manifest.yml records a source_revision
    And a symbol referenced by a source_link changed since that revision
    When the user runs `mdd map-status`
    Then it reports Stale and lists each drifted (path, symbol, @id)
    And it exits non-zero

  Scenario: Greenfield — no whole-map yet
    Given there is no .mdd/map/manifest.yml
    When the user runs `mdd map-status`
    Then it reports Fresh with no baseline (nothing to be stale against)

  Scenario: Reuses the traceability engine
    When map-status computes drift
    Then it uses the same git diff + syn symbol mapping as review pass 3
    And only the recorded source_revision baseline differs
