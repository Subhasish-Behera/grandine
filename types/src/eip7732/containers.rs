use bls::{AggregateSignatureBytes, PublicKeyBytes, SignatureBytes};
use serde::{Deserialize, Serialize};
use ssz::{BitVector, ContiguousList, Ssz};
use serde_utils;
use typenum::U4;

use crate::{
    deneb::containers::ExecutionPayload,
    deneb::primitives::KzgCommitment,
    eip7732::primitives::BuilderIndex,
    electra::containers::ExecutionRequests,
    phase0::primitives::{Epoch, ExecutionAddress, Gwei, Slot, ValidatorIndex, H256},
    preset::Preset,
};

// Core Execution Payload Containers

#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Serialize, Ssz)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPayloadHeader {
    pub parent_block_hash: H256,
    pub parent_block_root: H256,
    pub block_hash: H256,
    pub fee_recipient: ExecutionAddress,
    #[serde(with = "serde_utils::string_or_native")]
    pub gas_limit: u64,
    #[serde(with = "serde_utils::string_or_native")]
    pub builder_index: BuilderIndex,
    #[serde(with = "serde_utils::string_or_native")]
    pub slot: Slot,
    #[serde(with = "serde_utils::string_or_native")]
    pub value: Gwei,
    pub blob_kzg_commitments_root: H256,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Serialize, Ssz)]
#[serde(deny_unknown_fields)]
pub struct SignedExecutionPayloadHeader {
    pub message: ExecutionPayloadHeader,
    pub signature: SignatureBytes,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Serialize)]
#[serde(bound = "", deny_unknown_fields)]
pub struct ExecutionPayloadEnvelope<P: Preset> {
    pub payload: ExecutionPayload<P>,
    pub execution_requests: ExecutionRequests<P>,
    #[serde(with = "serde_utils::string_or_native")]
    pub builder_index: BuilderIndex,
    pub beacon_block_root: H256,
    #[serde(with = "serde_utils::string_or_native")]
    pub slot: Slot,
    pub blob_kzg_commitments: ContiguousList<KzgCommitment, P::MaxBlobCommitmentsPerBlock>,
    pub state_root: H256,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Serialize)]
#[serde(bound = "", deny_unknown_fields)]
pub struct SignedExecutionPayloadEnvelope<P: Preset> {
    pub message: ExecutionPayloadEnvelope<P>,
    pub signature: SignatureBytes,
}

// Payload Attestation Containers (PTC)

#[derive(Clone, PartialEq, Eq, Debug, Default, Deserialize, Serialize, Ssz)]
#[serde(deny_unknown_fields)]
pub struct PayloadAttestationData {
    pub beacon_block_root: H256,
    #[serde(with = "serde_utils::string_or_native")]
    pub slot: Slot,
    pub payload_present: bool,
    pub blob_data_available: bool,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Serialize, Ssz)]
#[serde(deny_unknown_fields)]
pub struct PayloadAttestationMessage {
    #[serde(with = "serde_utils::string_or_native")]
    pub validator_index: ValidatorIndex,
    pub data: PayloadAttestationData,
    pub signature: SignatureBytes,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Serialize)]
#[serde(bound = "", deny_unknown_fields)]
pub struct PayloadAttestation<P: Preset> {
    pub aggregation_bits: BitVector<P::PtcSize>,  // PTC_SIZE = 512
    pub data: PayloadAttestationData,
    pub signature: AggregateSignatureBytes,  // Aggregate signature from PTC members
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IndexedPayloadAttestation {
    #[serde(with = "serde_utils::string_or_native_sequence")]
    pub attesting_indices: ContiguousList<ValidatorIndex, U4>,  // MAX_PAYLOAD_ATTESTATIONS = 4
    pub data: PayloadAttestationData,
    pub signature: AggregateSignatureBytes,  // Aggregate signature from indexed validators
}

// Builder-Related Containers

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize, Serialize, Ssz)]
#[serde(deny_unknown_fields)]
pub struct BuilderPendingWithdrawal {
    pub fee_recipient: ExecutionAddress,
    #[serde(with = "serde_utils::string_or_native")]
    pub amount: Gwei,
    #[serde(with = "serde_utils::string_or_native")]
    pub builder_index: BuilderIndex,
    #[serde(with = "serde_utils::string_or_native")]
    pub withdrawable_epoch: Epoch,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize, Serialize, Ssz)]
#[serde(deny_unknown_fields)]
pub struct BuilderPendingPayment {
    #[serde(with = "serde_utils::string_or_native")]
    pub weight: Gwei,
    pub withdrawal: BuilderPendingWithdrawal,
}