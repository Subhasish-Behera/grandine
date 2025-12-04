use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use bls::AggregateSignature;
use ssz::BitList;
use tokio::sync::{Mutex, RwLock};
use types::{
    phase0::containers::{Attestation, AttestationData},
    phase0::primitives::{CommitteeIndex, Slot},
    preset::Preset,
};

// Use `Mutex` instead of `RwLock` to avoid race conditions in `InsertAttestationTask`.
// Don't let this comment fool you into thinking the locking is well thought out.
// There may be other bugs.
pub type AggregateMap<P> = HashMap<AttestationData, Arc<Mutex<Vec<Aggregate<P>>>>>;

pub type AttestationMap<P> = HashMap<AttestationData, Arc<RwLock<AttestationSet<P>>>>;

// Use `BTreeSet` to make attestation packing deterministic for snapshot testing.
// This does not affect performance in our benchmarks.
pub type AttestationSet<P> = BTreeSet<Arc<Attestation<P>>>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttestationPrePool {
    pub committee_index: CommitteeIndex,
    pub original_payload_index: u64,
    pub attestation_slot: Slot,
}

#[derive(Default, Clone)]
pub struct Aggregate<P: Preset> {
    pub aggregation_bits: BitList<P::MaxValidatorsPerCommittee>,
    pub signature: AggregateSignature,
}
