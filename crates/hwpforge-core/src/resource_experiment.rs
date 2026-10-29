//! Feature-only resource policy and observation for decoder experiments.
use std::cell::Cell;

/// When to enforce the additional budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timing {
    /// Limit work before the unit completes.
    Measured,
    /// Check after completing the unit.
    Post,
}
/// Lifetime of the policy counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Reset for each unit.
    Local,
    /// Accumulate over the document.
    Document,
}
/// Configuration installed before a decode call.
#[derive(Debug, Clone, Copy)]
pub struct Experiment {
    /// Enforcement time.
    pub timing: Timing,
    /// Counter lifetime.
    pub scope: Scope,
    /// Allowed output bytes.
    pub budget: u64,
}
/// Completed work, including overflow probe bytes for ZIP.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    /// Completed output bytes.
    pub q: u64,
    /// Completed bytes at the first budget rejection.
    pub first_reject_q: Option<u64>,
}
thread_local! {
    static CONFIG: Cell<Option<Experiment>> = const { Cell::new(None) };
    static OBSERVATION: Cell<Snapshot> = const { Cell::new(Snapshot { q: 0, first_reject_q: None }) };
    static ON_REJECT: Cell<Option<fn(Snapshot)>> = const { Cell::new(None) };
}
/// Install settings and reset observations on the current thread.
pub fn set_experiment(config: Experiment) {
    CONFIG.set(Some(config));
    OBSERVATION.set(Snapshot::default());
}
/// Remove settings and the rejection observer.
pub fn clear_experiment() {
    CONFIG.set(None);
    ON_REJECT.set(None);
}
/// Read settings at the instrumented ZIP or note-expansion site.
pub fn configuration() -> Option<Experiment> {
    CONFIG.get()
}
/// Observe completed work on this thread.
pub fn snapshot() -> Snapshot {
    OBSERVATION.get()
}
/// Install an observer called at the first rejection.
pub fn set_rejection_observer(observer: fn(Snapshot)) {
    ON_REJECT.set(Some(observer));
}
/// Publish completed work from the instrumented site.
pub fn record_read(n: u64) {
    OBSERVATION.with(|cell| {
        let mut s = cell.get();
        s.q = s.q.saturating_add(n);
        cell.set(s);
    });
}
/// Mark the first rejection before constructing its error.
pub fn reject() {
    let mut s = snapshot();
    if s.first_reject_q.is_none() {
        s.first_reject_q = Some(s.q);
        OBSERVATION.set(s);
        if let Some(observer) = ON_REJECT.get() {
            observer(s);
        }
    }
}
