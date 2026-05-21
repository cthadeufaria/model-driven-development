# AT-USE-SESSION-BRIEF — acceptance for USE-SESSION-BRIEF
Feature: Session-start brief surfaces the model TOC and freshness verdict
  As an agent starting a session in an mdd project
  I want a compact whole-map TOC plus the freshness verdict
  So that I begin oriented and know whether to trust the diagrams

  Scenario: mdd context prints a TOC and a freshness verdict
    Given an mdd project with an accumulated whole-map under .mdd/map
    When I run "mdd context"
    Then the output lists each concept kind with its concept and @id counts
    And the output ends with a single FRESH or STALE freshness verdict
    And the command exits 0

  Scenario: mdd context with no whole-map yet
    Given an mdd project with no .mdd/map directory
    When I run "mdd context"
    Then the output reports an empty map and a FRESH (no baseline) verdict
    And the command exits 0

  Scenario: the SessionStart hook injects the brief automatically
    Given mdd init has wired the SessionStart hook into .claude/settings.json
    When a Claude Code session starts
    Then "mdd context" runs and its output is injected as additionalContext
