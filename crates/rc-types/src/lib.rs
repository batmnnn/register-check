//! Register Check core types: the 7 register tables as Rust structs, their Arrow
//! schemas, and RecordBatch builders. Amounts are raw base units (u64); addresses
//! and signatures are base58 strings.

mod batch;
mod error;
mod events;
mod schema;

pub use batch::to_batches;
pub use error::TypesError;
pub use events::{
    AllowEntry, AuthorityChange, Balance, BurnEvent, ChangeKind, MintEvent, Register, Reserve,
    TransferEvent,
};
pub use schema::{schema, Table};
