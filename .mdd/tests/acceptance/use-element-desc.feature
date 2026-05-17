# @id(AT-USE-ELEMENT-DESC)
# @ref(USE-ELEMENT-DESC)
# Acceptance scaffold: selecting a diagram element shows its @desc text
# in the MODEL CONTEXT card, distinctly styled.
Feature: Read a diagram element's description

  Background:
    Given the mdd viewer is open
    And a model file whose @id elements carry @desc markers

  Scenario: Selecting an element shows its description
    When the developer selects an element / @id
    Then the MODEL CONTEXT card shows a distinct DESCRIPTION block
    And the block contains that element's @desc text

  Scenario: Element without a description
    Given a selected @id that has no @desc marker
    When the developer selects it
    Then the MODEL CONTEXT card shows a weak "No description" placeholder

  Scenario: Description is per element
    Given two elements with different @desc text in the same file
    When the developer selects each in turn
    Then the DESCRIPTION block updates to the selected element's text
