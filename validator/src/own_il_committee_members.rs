use std::sync::Arc;

use anyhow::Result;
use bls::PublicKeyBytes;
use helper_functions::accessors;
use logging::warn_with_peers;
use scc::HashMap as SccHashMap;
use signer::Signer;
use ssz::H256;
use std_ext::ArcExt as _;
use tap::{Conv as _, Pipe as _};
use tracing::instrument;
use types::{
    combined::BeaconState,
    phase0::primitives::{Slot, ValidatorIndex},
    preset::Preset,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ILCommitteeMember {
    pub public_key: PublicKeyBytes,
    pub validator_index: ValidatorIndex,
}

pub struct OwnILCommitteeMembers {
    signer: Arc<Signer>,
    members: SccHashMap<(H256, Slot), Arc<[ILCommitteeMember]>>,
}

impl OwnILCommitteeMembers {
    pub fn new(signer: Arc<Signer>) -> Self {
        Self {
            signer,
            members: SccHashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    #[instrument(skip_all, level = "debug", fields(slot = slot, dependent_root = ?dependent_root))]
    pub async fn get_or_init_at_slot<P: Preset>(
        &self,
        state: &BeaconState<P>,
        dependent_root: H256,
        slot: Slot,
    ) -> Option<Arc<[ILCommitteeMember]>> {
        if let Some(members) = self.members.get_async(&(dependent_root, slot)).await {
            return Some(members.clone_arc());
        }

        match self.compute_members_at_slot(state, slot) {
            Ok(members) => {
                if let Some(members) = members {
                    self.members
                        .upsert_async((dependent_root, slot), members.clone_arc())
                        .await;

                    Some(members)
                } else {
                    None
                }
            }
            Err(error) => {
                warn_with_peers!(
                    "failed to compute own inclusion list committee members at slot {slot}: {error:?}"
                );
                None
            }
        }
    }

    pub async fn prune(&self, up_to_slot: Slot) {
        self.members
            .retain_async(|(_, slot), _| *slot >= up_to_slot)
            .await
    }

    #[instrument(skip_all, level = "debug", fields(slot = slot))]
    fn compute_members_at_slot<P: Preset>(
        &self,
        state: &BeaconState<P>,
        slot: Slot,
    ) -> Result<Option<Arc<[ILCommitteeMember]>>> {
        let signer_snapshot = self.signer.load();

        let own_validators = signer_snapshot
            .keys()
            .copied()
            .filter_map(|public_key| {
                let validator_index = accessors::index_of_public_key(state, &public_key)?;
                Some((validator_index, public_key))
            })
            .collect::<Vec<_>>();

        if own_validators.is_empty() {
            return Ok(None);
        }

        let committee = accessors::get_inclusion_list_committee(state, slot)?;

        // The committee may repeat an index, but a validator has one inclusion list duty per slot,
        // so own validators are matched by membership rather than by committee position.
        // <https://github.com/ethereum/consensus-specs/blob/v1.7.0-beta.1/specs/heze/validator.md#inclusion-list-committee>
        own_validators
            .into_iter()
            .filter_map(|(validator_index, public_key)| {
                committee
                    .contains(&validator_index)
                    .then_some(ILCommitteeMember {
                        public_key,
                        validator_index,
                    })
            })
            .collect::<Vec<_>>()
            .conv::<Arc<[_]>>()
            .pipe(Some)
            .pipe(Ok)
    }
}
