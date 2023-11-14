use std::fmt::{Debug, Formatter};
use thiserror::Error;

#[derive(Clone, Error, PartialEq)]
pub enum DragoonError {
    #[error("Bad listener given")]
    BadListener,
}

impl Debug for DragoonError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self)?;
        Ok(())
    }
}
