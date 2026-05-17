# @id(AT-USE-RUN-CYCLE)
# @ref(USE-RUN-CYCLE)
# Acceptance scaffold: the single-description orchestration skill picks
# the right entry point, owns the cycle boundary, loops to parity, and
# pauses for clarification instead of guessing.
Feature: Run a full MDD cycle from one description

  Scenario: A description starts at generate and loops to parity
    Given a feature description is provided to /mdd-cycle
    When the skill runs
    Then it opens a cycle and snapshots .mdd/models/current/ into before/
    And it starts at /mdd-generate
    And it loops validate -> implement -> map -> validate -> review until parity matches
    And on a match it snapshots after/ and writes the diagram diffs and closes the cycle

  Scenario: No description behaves as /mdd-map with no comments
    Given no description is provided to /mdd-cycle
    When the skill runs
    Then it starts at /mdd-map with no comments
    And it does not run the implement/review loop

  Scenario: Ambiguity blocks on the user
    Given a modeling or implementation decision is genuinely ambiguous
    When the skill reaches that decision
    Then it pauses and asks the user a clarifying question
    And it does not guess or proceed until the user answers

  Scenario: The skill owns the cycle boundary
    Given a standalone /mdd-map or /mdd-generate run outside /mdd-cycle
    When that skill runs
    Then it does not open or close a cycle
    And only /mdd-cycle opens and closes cycles
