# @id(AT-USE-INIT-MIGRATE)
# @ref(USE-INIT-MIGRATE)
Feature: Forward-migrate state files (mdd init)
  So that config, trace, and approvals are updated for newer tool versions
  without being overwritten, `mdd init` migrates each state file forward
  when its on-disk version is older, preserves its content, and never
  downgrades a file written by a newer tool.

  Scenario: An older state file is migrated forward
    Given a config.yml whose version is older than the current schema version
    When the user runs `mdd init`
    Then the file is re-serialized through the current struct
    And fields added since the file was written are filled from defaults
    And the version is bumped to the current schema version
    And it is reported on a `migrated` line

  Scenario: Migration preserves accumulated content
    Given a trace.yml at an older version with many links and source_links
    When the user runs `mdd init`
    Then every prior link, generated test, and source_link is still present
    And only the version and any newly-defaulted fields changed

  Scenario: A current-version file is left untouched
    Given state files already at the current schema version
    When the user runs `mdd init`
    Then config.yml, trace.yml, and approvals.yml are not rewritten
    And running init again changes nothing (idempotent)

  Scenario: A newer state file is never downgraded
    Given a config.yml whose version is newer than the running tool
    When the user runs `mdd init`
    Then the file is left untouched
    And it is not downgraded

  Scenario: Migration ignores the --force flag
    Given an older trace.yml
    When the user runs `mdd init --force`
    Then the file is migrated forward, not overwritten from a template
