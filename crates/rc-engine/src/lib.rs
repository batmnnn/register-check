//! Register Check rule engine: registers the 7 register tables as DataFusion
//! MemTables, adds the `latest_reserve` and `supply` views, and runs the
//! static SQL rules in `rules/R<N>.sql`.

mod context;
mod error;
mod rules;

pub use context::{Engine, Violation};
pub use error::EngineError;
pub use rules::{load_rules, parse_rule, Rule};
