use serde::{Deserialize, Serialize};

pub(crate) mod random;
pub(crate) mod round_robin;

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub(crate) enum StrategyName {
    Random,
    RoundRobin,
}
