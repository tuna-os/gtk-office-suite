//! The error a journey fails with.

use dcp_core::StoreError;

/// A journey failed: the binding broke a rule of the spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure(pub String);

impl Failure {
    pub fn new(message: impl Into<String>) -> Self {
        Failure(message.into())
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Failure {}

impl From<StoreError> for Failure {
    fn from(e: StoreError) -> Self {
        Failure(format!("unexpected store error: {e}"))
    }
}

/// `ensure!(condition, "what {}", x)`: fail the journey unless it holds.
#[macro_export]
macro_rules! ensure {
    ($cond:expr, $($msg:tt)+) => {
        if !($cond) {
            return Err($crate::Failure::new(format!($($msg)+)));
        }
    };
}
