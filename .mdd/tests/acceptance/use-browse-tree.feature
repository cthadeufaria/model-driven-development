# @id(AT-USE-BROWSE-TREE)
# @ref(USE-BROWSE-TREE)
# Acceptance scaffold: the left rail is a VSCode-style collapsible
# directory tree, with an alternate group-by-cycle organization.
Feature: Browse models in a directory tree

  Background:
    Given the mdd viewer is open with mapped models

  Scenario: Rail shows a collapsible directory tree
    When the developer looks at the left rail
    Then model files appear as a nested directory tree
    And folders can be expanded and collapsed
    And there is no flat alphabetical list

  Scenario: Selecting a leaf loads the diagram
    When the developer clicks a diagram leaf in the tree
    Then that diagram loads in the canvas as before

  Scenario: Group by cycle
    Given at least one closed cycle exists
    When the developer switches the rail to "By cycle"
    Then diagrams are grouped under the cycle that last touched them
    And switching back restores the directory organization
