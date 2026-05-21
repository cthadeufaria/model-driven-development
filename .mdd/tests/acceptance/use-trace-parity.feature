# @id(AT-USE-TRACE-PARITY)
# @ref(USE-TRACE-PARITY)
Feature: Traceability parity (review pass 3)
  So that diagrams stay a trustworthy index into the code, the review gate
  fails when a diagram element points at code that doesn't exist, or when
  behaviour-bearing code is edited with no diagram counterpart.

  Background:
    Given a project whose cycle recorded a base_revision
    And Project::review() runs ID parity, then security parity, then traceability

  Scenario: Forward — a diagram element points at code that does not exist
    Given an implementable @id (CMP-/SEQ-/DOM-/STM-) with a source_link
    And the linked symbol cannot be found by the syn index for its file
    When review_traceability runs
    Then a forward error is reported for that (model_id, path, symbol)
    And the combined review gate does not match

  Scenario: Reverse bucket B — edited behaviour with no diagram counterpart
    Given a function added or modified since base_revision
    And no source_link covers that (path, symbol)
    And the symbol is not classified as escape-hatch glue
    When review_traceability runs
    Then it is reported in reverse bucket B as a blocking error
    And the combined review gate does not match

  Scenario: Reverse bucket A — edited glue is warned, not blocked
    Given a changed region that is an import, attribute, or comment
    And it has no diagram counterpart
    When review_traceability runs
    Then it is reported in reverse bucket A as a warning
    And it does not by itself fail the gate

  Scenario: Untouched code is never inspected
    Given a function that did not change since base_revision and has no source_link
    When review_traceability runs
    Then it produces no forward error and no reverse finding for that function

  Scenario: Clean change closes the cycle
    Given every implementable @id resolves to a real symbol
    And every changed behaviour-bearing symbol is covered by a source_link
    When review() runs
    Then ID parity, security parity, and traceability all match and the cycle closes
