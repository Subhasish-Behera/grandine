use hex_literal::hex;
use crate::phase0::primitives::{DomainType, H32};

// ePBS domain types for signing
pub const DOMAIN_BEACON_BUILDER: DomainType = H32(hex!("1B000000"));
pub const DOMAIN_PTC_ATTESTER: DomainType = H32(hex!("1C000000"));

// Builder withdrawal prefix
pub const BUILDER_WITHDRAWAL_PREFIX: &[u8] = &hex!("03");

// ePBS timing constants  
pub const PAYLOAD_TIMELY_THRESHOLD: u64 = 3; // slots
pub const PAYLOAD_REVEAL_DEADLINE: u64 = 2; // seconds into slot
pub const PAYLOAD_WITHHOLD_BOOST_FACTOR: u64 = 40; // percentage

// Payload status values (EIP-7732)
pub const PAYLOAD_ABSENT: u8 = 0;
pub const PAYLOAD_PRESENT: u8 = 1;
pub const PAYLOAD_WITHHELD: u8 = 2;
pub const PAYLOAD_INVALID_STATUS: u8 = 3;

// ePBS preset constants
pub const PTC_SIZE: u64 = 512;
pub const MAX_PAYLOAD_ATTESTATIONS: u64 = 4;