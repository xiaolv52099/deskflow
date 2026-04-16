use std::fmt;

pub struct Builder;

impl Builder {
    pub fn default() -> Self {
        Self
    }

    pub fn run(self, _context: Context) -> Result<(), RunError> {
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Context;

#[derive(Debug, Default)]
pub struct RunError;

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tauri stub run error")
    }
}

impl std::error::Error for RunError {}

#[macro_export]
macro_rules! generate_context {
    () => {
        $crate::Context::default()
    };
}

