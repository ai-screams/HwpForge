//! Work counting for the tests that pin a path's cost to its input size
//! (linear in fields, items, images …). The counters exist only in test
//! builds; elsewhere the calls compile to nothing.

/// Counts one unit of work.
#[inline]
pub(crate) fn tick() {
    tick_n(1);
}

/// Counts `n` units of work.
#[inline]
#[cfg_attr(not(test), allow(unused_variables))]
pub(crate) fn tick_n(n: usize) {
    #[cfg(test)]
    WORK.with(|w| w.set(w.get() + n));
}

#[cfg(test)]
thread_local! {
    /// Work units counted by [`tick`] on this thread.
    pub(crate) static WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
