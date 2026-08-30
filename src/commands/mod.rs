pub mod apply;
pub mod edit;
pub mod hydrate;
pub mod info;
pub mod plan;
pub mod update;
pub mod verify;
#[cfg(test)]
pub mod testing;

use crate::error::Failures;

/// What a command reports back to the process exit code.
#[derive(Debug, Default)]
pub struct Outcome {
    pub failures: Failures,
}

impl Outcome {
    pub fn ok() -> Self {
        Outcome::default()
    }

    pub fn succeeded(&self) -> bool {
        self.failures.is_empty()
    }
}
