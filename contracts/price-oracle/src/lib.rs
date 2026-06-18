#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;

pub mod aggregator;
pub mod errors;
pub mod events;
pub mod preimage;
pub mod signed_oracle;
pub mod types;

pub use aggregator::OracleAggregator;
pub use signed_oracle::SignedPriceOracle;
