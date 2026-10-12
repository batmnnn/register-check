//! Register Check ingestion: page a mint's finalized signatures, fetch each transaction as
//! jsonParsed, and turn Token-2022 instructions into register events. All HTTP sits behind
//! [`RpcSource`] so tests run on saved devnet responses.

mod error;
mod ingester;
mod parse;
mod reserve;
mod rpc;

pub use error::{IngestError, ReserveError};
pub use ingester::{BalanceDrift, Ingester, PollReport, SupplyDrift};
pub use parse::{parse_transaction, Event, MintState, ParsedTx, TOKEN_2022_PROGRAM_ID};
pub use reserve::{reserve_message, verify_reserve, MAX_FUTURE_SKEW_SECS};
pub use rpc::{HttpRpc, RpcSource, SigInfo, TokenSupply};
