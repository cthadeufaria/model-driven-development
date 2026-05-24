# @id(AT-USE-INIT-FORCE)
# @ref(USE-INIT-FORCE)
Feature: Force-overwrite regenerable scaffolding (mdd init --force)
  So that an existing workspace can be upgraded to the current tool's
  templates in one shot, `mdd init --force` overwrites every regenerable
  scaffolding file without the per-file prompt, while the three
  accumulating state files stay untouched.

  Scenario: --force overwrites regenerable files without prompting
    Given an initialized workspace whose docs and skill files were modified
    When the user runs `mdd init --force`
    Then each regenerable file (docs, workflow skills, CLAUDE.md/AGENTS.md
      blocks, SessionStart hook) is overwritten from the current templates
    And the user is not prompted per file
    And each overwrite is reported on an `overwrote` line

  Scenario: --force never overwrites the state files
    Given an initialized workspace with accumulated config, trace, and approvals
    When the user runs `mdd init --force`
    Then config.yml, trace.yml, and approvals.yml are not overwritten
    And their accumulated content is preserved

  Scenario: No flag keeps the interactive prompt
    Given an initialized workspace with an existing modified doc
    When the user runs `mdd init` without --force
    Then init prompts to overwrite or skip that file
    And skipping leaves the file unchanged
