//! Architecture source-of-truth engine (CMP-ARCH-CLI / DOM-ARCH-DIFF).
//!
//! Parses `.mdd/architecture/*.yml`, structurally diffs two specs, and checks
//! the `OCL-ARCH-*` invariants. Everything here is a **pure function** (no IO,
//! no git) so it unit-tests cleanly; the IO/git wrappers live on `Project`
//! (`load_architecture`, `arch_status`, `arch_diff`).

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Where the architecture source of truth lives.
pub const ARCH_DIR: &str = ".mdd/architecture";

/// The valid `decisions.yml` `status` values (DecisionStatus).
pub const DECISION_STATUSES: [&str; 4] = ["proposed", "accepted", "superseded", "deprecated"];

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Component {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub owns: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub tech: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Decision {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub decision: String,
    #[serde(default)]
    pub consequences: String,
    #[serde(default)]
    pub supersedes: Option<String>,
    #[serde(default)]
    pub superseded_by: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Constraint {
    pub id: String,
    #[serde(default)]
    pub rule: String,
    #[serde(default)]
    pub applies_to: Vec<String>,
    #[serde(default)]
    pub rationale: String,
}

#[derive(Debug, Default, Deserialize)]
struct ComponentsFile {
    #[serde(default)]
    components: Vec<Component>,
}
#[derive(Debug, Default, Deserialize)]
struct DecisionsFile {
    #[serde(default)]
    decisions: Vec<Decision>,
}
#[derive(Debug, Default, Deserialize)]
struct ConstraintsFile {
    #[serde(default)]
    constraints: Vec<Constraint>,
}

/// The parsed architecture source of truth (DOM-ARCH-SPEC).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ArchitectureSpec {
    pub components: Vec<Component>,
    pub decisions: Vec<Decision>,
    pub constraints: Vec<Constraint>,
}

impl ArchitectureSpec {
    /// Parse from the three file contents. An empty/whitespace string (a file
    /// that does not exist) is treated as no entries — unknown keys such as
    /// `version:` are ignored by serde, so the documented-but-empty templates
    /// parse to an empty spec.
    pub fn parse(components_yml: &str, decisions_yml: &str, constraints_yml: &str) -> Result<Self> {
        fn load<T: Default + for<'de> Deserialize<'de>>(yml: &str, what: &str) -> Result<T> {
            if yml.trim().is_empty() {
                Ok(T::default())
            } else {
                serde_yaml::from_str(yml).with_context(|| format!("parse {what}"))
            }
        }
        let components: ComponentsFile = load(components_yml, "components.yml")?;
        let decisions: DecisionsFile = load(decisions_yml, "decisions.yml")?;
        let constraints: ConstraintsFile = load(constraints_yml, "constraints.yml")?;
        Ok(Self {
            components: components.components,
            decisions: decisions.decisions,
            constraints: constraints.constraints,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.components.is_empty() && self.decisions.is_empty() && self.constraints.is_empty()
    }

    /// Counts for `mdd arch status`, including decisions by status.
    pub fn summary(&self) -> ArchSummary {
        let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
        for d in &self.decisions {
            *by_status.entry(d.status.clone()).or_default() += 1;
        }
        ArchSummary {
            components: self.components.len(),
            decisions: self.decisions.len(),
            constraints: self.constraints.len(),
            decisions_by_status: by_status,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ArchSummary {
    pub components: usize,
    pub decisions: usize,
    pub constraints: usize,
    pub decisions_by_status: BTreeMap<String, usize>,
}

/// A structured, semantic diff of two specs, partitioned by entity + id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ArchDiff {
    pub added_components: Vec<String>,
    pub removed_components: Vec<String>,
    pub changed_components: Vec<String>,
    pub added_decisions: Vec<String>,
    pub removed_decisions: Vec<String>,
    pub changed_decisions: Vec<String>,
    pub added_constraints: Vec<String>,
    pub removed_constraints: Vec<String>,
    pub changed_constraints: Vec<String>,
}

impl ArchDiff {
    pub fn is_empty(&self) -> bool {
        self.added_components.is_empty()
            && self.removed_components.is_empty()
            && self.changed_components.is_empty()
            && self.added_decisions.is_empty()
            && self.removed_decisions.is_empty()
            && self.changed_decisions.is_empty()
            && self.added_constraints.is_empty()
            && self.removed_constraints.is_empty()
            && self.changed_constraints.is_empty()
    }
}

/// Partition two id-keyed lists into (added, removed, changed) id sets.
/// `added` = in head not base; `removed` = in base not head; `changed` =
/// present in both but not equal. All sorted + de-duplicated.
fn partition_by_id<T: PartialEq>(
    base: &[T],
    head: &[T],
    id_of: impl Fn(&T) -> String,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let base_map: BTreeMap<String, &T> = base.iter().map(|x| (id_of(x), x)).collect();
    let head_map: BTreeMap<String, &T> = head.iter().map(|x| (id_of(x), x)).collect();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for (k, hv) in &head_map {
        match base_map.get(k) {
            None => added.push(k.clone()),
            Some(bv) => {
                if *bv != *hv {
                    changed.push(k.clone());
                }
            }
        }
    }
    for k in base_map.keys() {
        if !head_map.contains_key(k) {
            removed.push(k.clone());
        }
    }
    (added, removed, changed)
}

/// Structurally diff `base` -> `head` (DOM-ARCH-DIFF). Pure.
pub fn diff(base: &ArchitectureSpec, head: &ArchitectureSpec) -> ArchDiff {
    let (added_components, removed_components, changed_components) =
        partition_by_id(&base.components, &head.components, |c| c.id.clone());
    let (added_decisions, removed_decisions, changed_decisions) =
        partition_by_id(&base.decisions, &head.decisions, |d| d.id.clone());
    let (added_constraints, removed_constraints, changed_constraints) =
        partition_by_id(&base.constraints, &head.constraints, |c| c.id.clone());
    ArchDiff {
        added_components,
        removed_components,
        changed_components,
        added_decisions,
        removed_decisions,
        changed_decisions,
        added_constraints,
        removed_constraints,
        changed_constraints,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ViolationKind {
    /// OCL-ARCH-DECISION-HAS-STATUS: status missing or not a known value.
    DecisionMissingStatus,
    /// OCL-ARCH-SUPERSEDE-LINKED: a superseded decision names no successor.
    SupersededMissingSuccessor,
    /// OCL-ARCH-COMPONENTS-IN-SYNC: an owns/depends_on id is not a model @id.
    ComponentRefUnresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArchViolation {
    pub kind: ViolationKind,
    pub subject: String,
    pub message: String,
}

/// Check the `OCL-ARCH-*` invariants over a spec (DOM-ARCH-DIFF). Pure:
/// `valid_model_ids` is the set of diagram `@id`s a component may reference.
/// Backs both `mdd arch status` (blocking) and the `mdd validate` WARNING rules.
pub fn check(spec: &ArchitectureSpec, valid_model_ids: &BTreeSet<String>) -> Vec<ArchViolation> {
    let mut violations = Vec::new();
    for d in &spec.decisions {
        if !DECISION_STATUSES.contains(&d.status.as_str()) {
            violations.push(ArchViolation {
                kind: ViolationKind::DecisionMissingStatus,
                subject: d.id.clone(),
                message: format!(
                    "decision {} has invalid or missing status {:?} (expected one of {:?})",
                    d.id, d.status, DECISION_STATUSES
                ),
            });
        }
        if d.status == "superseded"
            && d.superseded_by.as_deref().unwrap_or("").trim().is_empty()
        {
            violations.push(ArchViolation {
                kind: ViolationKind::SupersededMissingSuccessor,
                subject: d.id.clone(),
                message: format!("superseded decision {} has no superseded_by", d.id),
            });
        }
    }
    for c in &spec.components {
        for referenced in c.owns.iter().chain(c.depends_on.iter()) {
            if !valid_model_ids.contains(referenced) {
                violations.push(ArchViolation {
                    kind: ViolationKind::ComponentRefUnresolved,
                    subject: c.id.clone(),
                    message: format!(
                        "component {} references unknown model @id {}",
                        c.id, referenced
                    ),
                });
            }
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(decisions_yml: &str, components_yml: &str) -> ArchitectureSpec {
        ArchitectureSpec::parse(components_yml, decisions_yml, "").unwrap()
    }

    #[test]
    fn parse_empty_template_is_empty() {
        let s = ArchitectureSpec::parse(
            "version: 1\ncomponents: []\n",
            "version: 1\ndecisions: []\n",
            "version: 1\nconstraints: []\n",
        )
        .unwrap();
        assert!(s.is_empty());
        // A missing file (empty string) parses to no entries too.
        assert!(ArchitectureSpec::parse("", "", "").unwrap().is_empty());
    }

    #[test]
    fn diff_partitions_added_removed_changed() {
        let base = spec(
            "decisions:\n  - id: AD-0001\n    title: A\n    status: accepted\n  - id: AD-0002\n    title: B\n    status: accepted\n",
            "",
        );
        let head = spec(
            "decisions:\n  - id: AD-0001\n    title: A\n    status: accepted\n  - id: AD-0002\n    title: B changed\n    status: accepted\n  - id: AD-0003\n    title: C\n    status: accepted\n",
            "",
        );
        let d = diff(&base, &head);
        assert_eq!(d.added_decisions, vec!["AD-0003".to_string()]);
        assert!(d.removed_decisions.is_empty());
        assert_eq!(d.changed_decisions, vec!["AD-0002".to_string()]);
        assert!(!d.is_empty());
    }

    #[test]
    fn diff_of_identical_specs_is_empty() {
        let s = spec("decisions:\n  - id: AD-0001\n    status: accepted\n", "");
        assert!(diff(&s, &s).is_empty());
    }

    #[test]
    fn check_flags_bad_status_and_unlinked_supersede() {
        let s = spec(
            "decisions:\n  - id: AD-0001\n    status: bogus\n  - id: AD-0002\n    status: superseded\n",
            "",
        );
        let v = check(&s, &BTreeSet::new());
        assert!(v
            .iter()
            .any(|x| x.kind == ViolationKind::DecisionMissingStatus && x.subject == "AD-0001"));
        assert!(v
            .iter()
            .any(|x| x.kind == ViolationKind::SupersededMissingSuccessor && x.subject == "AD-0002"));
    }

    #[test]
    fn check_flags_unresolved_component_ref_and_passes_when_resolved() {
        let s = spec(
            "",
            "components:\n  - id: CMP-API\n    owns: [DOM-TASK]\n    depends_on: [CMP-DB]\n",
        );
        // Nothing in the model registry -> both refs unresolved.
        let none = check(&s, &BTreeSet::new());
        assert_eq!(
            none.iter()
                .filter(|x| x.kind == ViolationKind::ComponentRefUnresolved)
                .count(),
            2
        );
        // Both ids known -> no ref violations.
        let known: BTreeSet<String> =
            ["DOM-TASK".to_string(), "CMP-DB".to_string()].into_iter().collect();
        assert!(check(&s, &known)
            .iter()
            .all(|x| x.kind != ViolationKind::ComponentRefUnresolved));
    }
}
