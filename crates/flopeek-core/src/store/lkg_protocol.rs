//! Facade for the Last-Known-Good Protocol 1.0 SQLite adapter.
//!
//! The implementation is kept behind a small public surface so command/query
//! code cannot accidentally become part of the store facade.  The domain
//! reducer remains in `model::lkg_protocol` and has no persistence dependency.

use super::*;

mod implementation {
    use super::*;
    include!("lkg_protocol_impl.rs");
}

pub use implementation::{
    confirm_last_known_good_local, get_last_known_good_protocol, get_last_known_good_review_packet,
    list_last_known_good_protocol, propose_last_known_good, reject_last_known_good_local,
    revoke_last_known_good_local, validate_last_known_good_protocol,
};

pub(crate) use implementation::confirmed_protocol_candidate;
