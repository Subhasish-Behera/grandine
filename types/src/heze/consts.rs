use hex_literal::hex;

use crate::phase0::primitives::{DomainType, H32};

// Domain types
pub const DOMAIN_INCLUSION_LIST_COMMITTEE: DomainType = H32(hex!("10000000"));
