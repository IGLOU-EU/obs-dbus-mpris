//! Online covers (B-18): what to look up for a track, and how to read
//! what the services answer. Pure, so it all runs under `cargo test`
//! without a network: the requests themselves are made by
//! [`crate::network`].

pub mod cleanup;
