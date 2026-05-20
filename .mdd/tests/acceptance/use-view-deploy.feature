# @id(AT-USE-VIEW-DEPLOY)
# @ref(USE-VIEW-DEPLOY)
# Acceptance scaffold: `mdd view` surfaces .mdd/deploy/**/*.puml in a
# dedicated DEPLOY rail section that is OUTSIDE the parity gate.
Feature: View deployment diagrams in the app

  Background:
    Given the viewer is open on a project that has .mdd/deploy/**/*.puml

  Scenario: Deployment diagrams appear in a dedicated DEPLOY section
    Given .mdd/deploy/azure-container-apps/diagram.puml exists
    When the left rail is shown
    Then a "DEPLOY" section lists deploy/azure-container-apps/diagram.puml
    And the section is labelled as a utility outside the parity gate

  Scenario: The DEPLOY section is visible in both rail modes
    Given the rail is in Directory mode
    Then the DEPLOY section is visible
    When the rail is switched to By-cycle mode
    Then the DEPLOY section is still visible (it is not cycle-scoped)

  Scenario: Selecting a deployment diagram paints its rendered SVG
    Given .mdd/rendered/deploy/azure-container-apps/diagram.svg exists
    When the developer clicks the deploy diagram leaf
    Then the canvas paints that SVG with the normal pan and zoom

  Scenario: Deploy diagram without a rendered SVG
    Given the deploy diagram has not been rendered
    Then the canvas shows the same "not rendered" placeholder as any model

  Scenario: Deployment diagrams never enter the parity gate
    Given the project has .mdd/deploy/**/*.puml
    When /mdd-validate or /mdd-review runs
    Then deployment ids are absent from the model registry
    And they affect neither ID parity nor security parity
