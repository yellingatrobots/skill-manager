//! How one entry differs between two states, for reporting what a command changed.

/// How one entry differs between two states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change<T> {
    Added(T),
    Removed(T),
    Changed(T, T),
    Unchanged(T),
}

impl<T> Change<T> {
    pub fn is_change(&self) -> bool {
        !matches!(self, Change::Unchanged(_))
    }
}
