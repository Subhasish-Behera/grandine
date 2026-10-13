use ssz::ProgressiveList;

use crate::{
    deneb::primitives::KzgCommitment,
    heze::containers::{InclusionListIdentifier, SignedExecutionPayloadBid, SignedInclusionList},
    preset::Preset,
};

impl<P: Preset> SignedExecutionPayloadBid<P> {
    #[must_use]
    pub const fn blob_kzg_commitments(&self) -> &ProgressiveList<KzgCommitment> {
        &self.message.blob_kzg_commitments
    }
}

impl<P: Preset> From<&SignedInclusionList<P>> for InclusionListIdentifier {
    fn from(signed_inclusion_list: &SignedInclusionList<P>) -> Self {
        let message = &signed_inclusion_list.message;

        Self {
            slot: message.slot,
            dependent_root: message.dependent_root,
            validator_index: message.validator_index,
        }
    }
}
