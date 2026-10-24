//! Running the journeys: by name, all of them, or a chosen set, with
//! capability-driven skipping that is always reported.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use crate::fixture::{Capabilities, StoreFactory};
use crate::{journeys, Failure};

pub type BoxFuture<'a> = Pin<Box<dyn Future<Output = Result<(), Failure>> + Send + 'a>>;

/// A capability a journey needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    GrantRevoke,
    Revocation,
    LeaseWrites,
    History,
    ArbitraryParents,
    Updates,
    UploadFaults,
    Forgery,
}

impl Need {
    pub fn met(self, c: &Capabilities) -> bool {
        match self {
            Need::GrantRevoke => c.grant_revoke,
            Need::Revocation => c.grant_revoke && c.revocation,
            Need::LeaseWrites => c.arbitrary_lease_writes,
            Need::History => c.history,
            Need::ArbitraryParents => c.arbitrary_parents,
            Need::Updates => c.updates,
            Need::UploadFaults => c.upload_faults,
            Need::Forgery => c.forgery,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Need::GrantRevoke => "grant_revoke",
            Need::Revocation => "revocation",
            Need::LeaseWrites => "arbitrary_lease_writes",
            Need::History => "history",
            Need::ArbitraryParents => "arbitrary_parents",
            Need::Updates => "updates",
            Need::UploadFaults => "upload_faults",
            Need::Forgery => "forgery",
        }
    }
}

pub struct Journey {
    /// The function name in [`crate::journeys`].
    pub name: &'static str,
    /// The row of spec section 12.
    pub spec_test: &'static str,
    pub needs: &'static [Need],
    pub run: for<'a> fn(&'a dyn StoreFactory) -> BoxFuture<'a>,
}

macro_rules! journey {
    ($name:ident, $spec:expr, $needs:expr) => {
        Journey {
            name: stringify!($name),
            spec_test: $spec,
            needs: $needs,
            run: |f| Box::pin(journeys::$name(f)),
        }
    };
}

/// Every journey, in spec order.
pub const JOURNEYS: &[Journey] = &[
    journey!(convergence_on_one_head, "1", &[]),
    journey!(
        second_head_shown_not_hidden,
        "2",
        &[Need::ArbitraryParents, Need::History]
    ),
    journey!(corrupt_file_rejected, "3", &[]),
    journey!(truncated_upload_never_published, "3", &[Need::UploadFaults]),
    journey!(lease_exclusion_and_readback, "4", &[Need::LeaseWrites]),
    journey!(lease_expiry_releases, "5", &[Need::LeaseWrites]),
    journey!(roles_enforced_at_store, "6", &[]),
    journey!(
        forged_version_from_a_viewer_is_not_accepted,
        "6",
        &[Need::Forgery]
    ),
    journey!(
        revocation_hides_new_content,
        "7",
        &[Need::GrantRevoke, Need::Revocation]
    ),
    journey!(
        unknown_algorithm_falls_back_to_versions,
        "8",
        &[Need::Updates]
    ),
    journey!(update_stream_gap_recovery, "9", &[Need::Updates]),
];

pub fn journey_names() -> Vec<&'static str> {
    JOURNEYS.iter().map(|j| j.name).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    Failed(Failure),
    /// A capability the journey needs is declared absent. Not a pass.
    Skipped {
        missing: Vec<&'static str>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JourneyResult {
    pub name: &'static str,
    pub spec_test: &'static str,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub binding: String,
    pub results: Vec<JourneyResult>,
}

impl Report {
    pub fn get(&self, name: &str) -> Option<&JourneyResult> {
        self.results.iter().find(|r| r.name == name)
    }

    pub fn passed(&self) -> Vec<&'static str> {
        self.names(|o| matches!(o, Outcome::Passed))
    }

    pub fn failed(&self) -> Vec<&'static str> {
        self.names(|o| matches!(o, Outcome::Failed(_)))
    }

    pub fn skipped(&self) -> Vec<&'static str> {
        self.names(|o| matches!(o, Outcome::Skipped { .. }))
    }

    fn names(&self, keep: impl Fn(&Outcome) -> bool) -> Vec<&'static str> {
        self.results
            .iter()
            .filter(|r| keep(&r.outcome))
            .map(|r| r.name)
            .collect()
    }

    /// Panic unless no journey failed. Skips do not fail this; check them
    /// with [`Self::assert_skipped_exactly`] so a binding states what it lacks.
    pub fn assert_no_failures(&self) {
        let failed: Vec<String> = self
            .results
            .iter()
            .filter_map(|r| match &r.outcome {
                Outcome::Failed(f) => {
                    Some(format!("  {} (spec test {}): {f}", r.name, r.spec_test))
                }
                _ => None,
            })
            .collect();
        assert!(
            failed.is_empty(),
            "{} failed conformance journeys:\n{}\n\n{self}",
            self.binding,
            failed.join("\n")
        );
    }

    /// Panic unless exactly the named journeys were skipped (by name). A
    /// binding calls this with the list its results document gives, so a skip
    /// can neither appear nor disappear unnoticed.
    pub fn assert_skipped_exactly(&self, expected: &[&str]) {
        let mut got = self.skipped();
        got.sort_unstable();
        let mut want: Vec<&str> = expected.to_vec();
        want.sort_unstable();
        assert_eq!(
            got, want,
            "{}: the skipped journeys differ from the declared ones\n{self}",
            self.binding
        );
    }

    /// No failures, no skips: every journey ran and passed.
    pub fn assert_all_passed(&self) {
        self.assert_no_failures();
        self.assert_skipped_exactly(&[]);
        assert_eq!(self.passed().len(), self.results.len());
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "DCP conformance, binding {}:", self.binding)?;
        for r in &self.results {
            match &r.outcome {
                Outcome::Passed => writeln!(f, "  PASS    {} (spec test {})", r.name, r.spec_test)?,
                Outcome::Failed(e) => {
                    writeln!(f, "  FAIL    {} (spec test {}): {e}", r.name, r.spec_test)?
                }
                Outcome::Skipped { missing } => writeln!(
                    f,
                    "  SKIPPED {} (spec test {}): binding lacks {}",
                    r.name,
                    r.spec_test,
                    missing.join(", ")
                )?,
            }
        }
        write!(
            f,
            "  {} passed, {} failed, {} skipped",
            self.passed().len(),
            self.failed().len(),
            self.skipped().len()
        )
    }
}

async fn run_journey(factory: &dyn StoreFactory, j: &Journey) -> JourneyResult {
    let caps = factory.capabilities();
    let missing: Vec<&'static str> = j
        .needs
        .iter()
        .filter(|n| !n.met(&caps))
        .map(|n| n.label())
        .collect();
    let outcome = if !missing.is_empty() {
        eprintln!(
            "SKIPPED {} on {}: binding lacks {}",
            j.name,
            factory.name(),
            missing.join(", ")
        );
        Outcome::Skipped { missing }
    } else {
        match (j.run)(factory).await {
            Ok(()) => Outcome::Passed,
            Err(e) => Outcome::Failed(e),
        }
    };
    JourneyResult {
        name: j.name,
        spec_test: j.spec_test,
        outcome,
    }
}

/// Run the journeys named in `names`, in spec order. An unknown name panics:
/// a typo must not silently select nothing.
pub async fn run_selected(factory: &dyn StoreFactory, names: &[&str]) -> Report {
    for n in names {
        assert!(
            JOURNEYS.iter().any(|j| j.name == *n),
            "no journey named {n}; known: {:?}",
            journey_names()
        );
    }
    let mut results = Vec::new();
    for j in JOURNEYS.iter().filter(|j| names.contains(&j.name)) {
        results.push(run_journey(factory, j).await);
    }
    Report {
        binding: factory.name(),
        results,
    }
}

/// Run every journey against `factory`.
pub async fn run_all(factory: &dyn StoreFactory) -> Report {
    let mut results = Vec::new();
    for j in JOURNEYS {
        results.push(run_journey(factory, j).await);
    }
    let report = Report {
        binding: factory.name(),
        results,
    };
    eprintln!("{report}");
    report
}

/// Run one journey by name (even if the binding lacks a capability it needs
/// that is reported as `Skipped`).
pub async fn run_one(factory: &dyn StoreFactory, name: &str) -> JourneyResult {
    let j = JOURNEYS
        .iter()
        .find(|j| j.name == name)
        .unwrap_or_else(|| panic!("no journey named {name}"));
    run_journey(factory, j).await
}
