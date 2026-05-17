# @id(AT-USE-CYCLE-DIFF-VIEW)
# @ref(USE-CYCLE-DIFF-VIEW)
# Acceptance scaffold: the superposed before/after diff view shows shared
# elements once, additions green, removals red.
Feature: View a cycle's before/after diff

  Background:
    Given a closed cycle with before/ and after/ snapshots

  Scenario: Superposed diff colors additions and removals
    When the developer opens the Diff view for that cycle
    Then elements present in both snapshots are drawn once, neutral
    And elements only in after/ are drawn green
    And elements only in before/ are drawn red

  Scenario: Buckets are disjoint
    When the diff is computed
    Then no element appears in more than one of unchanged, added, removed

  Scenario: Open cycle has no diff yet
    Given a cycle that is still open (no after/ snapshot)
    When the developer opens the Diff view
    Then a placeholder explains the diff is available after the cycle closes
