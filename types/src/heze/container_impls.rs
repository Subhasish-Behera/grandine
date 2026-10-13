use ssz::ProgressiveList;

use crate::{
    deneb::primitives::KzgCommitment, heze::containers::SignedExecutionPayloadBid, preset::Preset,
};

impl<P: Preset> SignedExecutionPayloadBid<P> {
    #[must_use]
    pub const fn blob_kzg_commitments(&self) -> &ProgressiveList<KzgCommitment> {
        &self.message.blob_kzg_commitments
    }
}
