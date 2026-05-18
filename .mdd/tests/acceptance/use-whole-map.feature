# @id(AT-USE-WHOLE-MAP)
# @ref(USE-WHOLE-MAP)
# Acceptance scaffold: /mdd-cycle's Close step accumulates each cycle's
# diff into a persisted, provenance-tagged .mdd/map/ baseline that nets
# add-then-remove to neither, snapshots it per cycle, and stays outside
# the parity gate.
Feature: Accumulate a whole-map baseline across cycles

  Scenario: Closing a cycle folds its diff into the whole-map
    Given a cycle N has reached parity and written its <diagram>.diff.puml
    When /mdd-cycle runs the Close step
    Then for each touched concept the added @ids are copied into .mdd/map/<kind>/<name>.puml
    And each newly added @id carries a ' @cycle(<ID>,N) provenance marker
    And @ids removed this cycle are deleted from the whole-map
    And unchanged @ids keep their earlier @cycle provenance
    And .mdd/map/manifest.yml records version, last_cycle=N, generated_at and files

  Scenario: Add-then-remove nets to neither
    Given an @id was added by an earlier cycle and removed by a later closed cycle
    When the whole-map has been accumulated through the later cycle
    Then that @id is absent from .mdd/map/ entirely
    And it is not re-injected as a removed ghost

  Scenario: Per-cycle history is snapshotted
    Given the whole-map has been accumulated for cycle N
    When the Close step finishes
    Then the whole .mdd/map/ tree is copied to .mdd/cycles/N/whole/
    And /mdd-render rasterizes .mdd/map/**.puml to .mdd/rendered/map/**.svg

  Scenario: The whole-map is outside the parity gate
    Given a populated .mdd/map/ tree
    When /mdd-validate, /mdd-review or the /mdd-cycle parity loop run
    Then none of them read or gate on .mdd/map/
