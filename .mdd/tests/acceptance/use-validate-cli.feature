# @id(AT-USE-VALIDATE-CLI)
# @ref(USE-VALIDATE-CLI)
Feature: Structural validation gate (mdd validate)
  So that the /mdd-validate skill runs a deterministic engine instead of
  re-deriving the checklist by inspection, the structural gate is reachable
  as a CLI command that exits non-zero on a blocking error.

  Scenario: Clean models pass
    Given a project whose current and objective models are structurally sound
    When the user runs `mdd validate`
    Then it prints VALIDATION: PASSED
    And it exits zero

  Scenario: A structural error blocks
    Given a project with a duplicate @id on one side
    When the user runs `mdd validate`
    Then it prints the offending error
    And it prints VALIDATION: FAILED
    And it exits non-zero

  Scenario: Warnings do not block
    Given a project that validates but has a stale approval or missing SVG
    When the user runs `mdd validate`
    Then the readiness warnings are reported
    And it still exits zero

  Scenario: Machine-readable output
    When the user runs `mdd validate --json`
    Then it prints a slim {ok, errors, warnings} JSON object
    And the full model registry is excluded from the output

  Scenario: Independent of review
    When the user runs `mdd validate`
    Then it does not run the review parity gate
