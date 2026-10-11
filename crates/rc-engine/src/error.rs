use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("reading rules from {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("rule file {file}: {reason}")]
    Rule { file: String, reason: String },
    #[error("unknown rule id {0}")]
    UnknownRule(String),
    #[error("rule {rule} failed: {source}")]
    Query {
        rule: String,
        #[source]
        source: datafusion::error::DataFusionError,
    },
    #[error(transparent)]
    DataFusion(#[from] datafusion::error::DataFusionError),
    #[error(transparent)]
    Types(#[from] rc_types::TypesError),
    #[error("encoding rows of {rule} as JSON: {reason}")]
    Json { rule: String, reason: String },
}
