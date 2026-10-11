use thiserror::Error;

#[derive(Debug, Error)]
pub enum TypesError {
    #[error("invalid register JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("arrow error building {table}: {source}")]
    Arrow {
        table: &'static str,
        #[source]
        source: arrow::error::ArrowError,
    },
}
