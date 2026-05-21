# @id(AT-USE-VERIFY-SOURCE-LINK)
# @ref(USE-VERIFY-SOURCE-LINK)
Feature: Source-link existence in validate
  So that /mdd-validate enforces what its checklist already claims (item 5),
  every trace.yml source_link must resolve to a real file and symbol; a
  dangling link is a structural error caught continuously, not only at review.

  Scenario: Missing file
    Given a source_link whose path does not exist in the repository
    When Project::validate() runs
    Then it reports a structural error naming that source_link
    And validation does not pass

  Scenario: Missing symbol
    Given a source_link whose path exists but whose symbol the syn index cannot find
    When Project::validate() runs
    Then it reports a structural error naming that (path, symbol)
    And validation does not pass

  Scenario: File-only link with no symbol
    Given a source_link with a path that exists and no symbol
    When Project::validate() runs
    Then the link passes the existence check

  Scenario: Resolvable link
    Given a source_link whose path exists and whose symbol the syn index finds
    When Project::validate() runs
    Then the link passes the existence check
