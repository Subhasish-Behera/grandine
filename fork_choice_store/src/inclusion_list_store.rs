//! Received inclusion lists.
//!
//! Model: `(slot, dependent_root) -> validator_index -> entry` (the list and whether it arrived
//! timely), plus the equivocators per key. The first list per validator is kept; a differing later
//! one marks the validator an equivocator, and equivocators are left out of every read.
//!
//! It is a field of `Store` and is changed by the mutator. Entries below the finalized slot
//! are pruned when finalization advances.
//!
//! In the spec this is the single global `InclusionListStore`; here it lives in `Store`.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use ssz::{BitVector, SszHash as _};
use typenum::Unsigned as _;
use types::{
    InclusionListCommittee,
    gloas::primitives::Transaction,
    heze::containers::SignedInclusionList,
    phase0::primitives::{H256, Slot, ValidatorIndex},
    preset::Preset,
};

/// [`InclusionListEntry`](https://github.com/ethereum/consensus-specs/blob/v1.7.0-beta.1/specs/heze/inclusion-list.md#inclusionlistentry)
#[derive(Clone)]
struct InclusionListEntry<P: Preset> {
    signed_inclusion_list: Arc<SignedInclusionList<P>>,
    timely: bool,
}

/// [`InclusionListStore`](https://github.com/ethereum/consensus-specs/blob/v1.7.0-beta.1/specs/heze/inclusion-list.md#inclusionliststore)
#[derive(Clone)]
pub struct InclusionListStore<P: Preset> {
    inclusion_lists: HashMap<(Slot, H256), HashMap<ValidatorIndex, InclusionListEntry<P>>>,
    /// Validators that published two differing lists under a key; excluded from every read.
    equivocators: HashMap<(Slot, H256), HashSet<ValidatorIndex>>,
}

impl<P: Preset> Default for InclusionListStore<P> {
    fn default() -> Self {
        Self {
            inclusion_lists: HashMap::new(),
            equivocators: HashMap::new(),
        }
    }
}

impl<P: Preset> InclusionListStore<P> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// [`process_inclusion_list`](https://github.com/ethereum/consensus-specs/blob/v1.7.0-beta.1/specs/heze/inclusion-list.md#new-process_inclusion_list)
    pub fn process_inclusion_list(
        &mut self,
        signed_inclusion_list: Arc<SignedInclusionList<P>>,
        timely: bool,
    ) {
        let slot = signed_inclusion_list.message.slot;
        let key = (slot, signed_inclusion_list.message.dependent_root);
        let validator_index = signed_inclusion_list.message.validator_index;

        if let Some(stored) = self
            .inclusion_lists
            .get(&key)
            .and_then(|by_validator| by_validator.get(&validator_index))
        {
            if stored.signed_inclusion_list.message != signed_inclusion_list.message {
                self.equivocators
                    .entry(key)
                    .or_default()
                    .insert(validator_index);
            }
            return;
        }

        self.inclusion_lists.entry(key).or_default().insert(
            validator_index,
            InclusionListEntry {
                signed_inclusion_list,
                timely,
            },
        );
    }

    /// [`get_inclusion_list_transactions`](https://github.com/ethereum/consensus-specs/blob/v1.7.0-beta.1/specs/heze/inclusion-list.md#new-get_inclusion_list_transactions)
    ///
    /// Transactions are deduplicated by tree-hash root.
    #[must_use]
    pub fn get_inclusion_list_transactions(
        &self,
        slot: Slot,
        dependent_root: H256,
        only_timely: bool,
    ) -> Vec<Transaction<P>> {
        let key = (slot, dependent_root);
        let Some(by_validator) = self.inclusion_lists.get(&key) else {
            return Vec::new();
        };
        let equivocators = self.equivocators.get(&key);

        let mut seen = HashSet::new();
        let mut transactions = Vec::new();

        for (validator_index, entry) in by_validator {
            if only_timely && !entry.timely {
                continue;
            }
            if equivocators.is_some_and(|set| set.contains(validator_index)) {
                continue;
            }
            for transaction in &entry.signed_inclusion_list.message.transactions {
                if seen.insert(transaction.hash_tree_root()) {
                    transactions.push(transaction.clone());
                }
            }
        }

        transactions
    }

    /// [`get_inclusion_list_bits`](https://github.com/ethereum/consensus-specs/blob/v1.7.0-beta.1/specs/heze/inclusion-list.md#new-get_inclusion_list_bits)
    #[must_use]
    pub fn get_inclusion_list_bits(
        &self,
        committee: &InclusionListCommittee<P>,
        slot: Slot,
        dependent_root: H256,
        only_timely: bool,
    ) -> BitVector<P::InclusionListCommitteeSize> {
        let key = (slot, dependent_root);
        let Some(by_validator) = self.inclusion_lists.get(&key) else {
            return BitVector::default();
        };
        let equivocators = self.equivocators.get(&key);
        let mut bits = BitVector::default();

        for (position, &validator_index) in committee.into_iter().enumerate() {
            let should_set = by_validator.get(&validator_index).is_some_and(|entry| {
                (!only_timely || entry.timely)
                    && !equivocators.is_some_and(|set| set.contains(&validator_index))
            });

            if should_set {
                bits.set(position, true);
            }
        }

        bits
    }

    /// Stored lists of the committee members at the set positions of `indices`, for an
    /// `InclusionListsByIndices` response. Skips equivocators and members without a stored list. A
    /// validator filling several positions (small committees wrap around) is returned once.
    #[must_use]
    pub fn signed_inclusion_lists_by_indices(
        &self,
        committee: &InclusionListCommittee<P>,
        slot: Slot,
        dependent_root: H256,
        indices: &BitVector<P::InclusionListCommitteeSize>,
    ) -> Vec<Arc<SignedInclusionList<P>>> {
        let key = (slot, dependent_root);
        let Some(by_validator) = self.inclusion_lists.get(&key) else {
            return Vec::new();
        };
        let equivocators = self.equivocators.get(&key);

        let requested = committee
            .iter()
            .zip(*indices)
            .filter(|(_, requested)| *requested)
            .map(|(validator_index, _)| *validator_index)
            .collect::<HashSet<_>>();

        requested
            .into_iter()
            // > Clients SHOULD NOT respond with inclusion lists from equivocators for the requested
            // > `slot` and `dependent_root`.
            // Spec <https://github.com/ethereum/consensus-specs/blob/v1.7.0-beta.1/specs/heze/p2p-interface.md#inclusionlistsbyindices-v1>.
            .filter(|validator_index| {
                !equivocators.is_some_and(|set| set.contains(validator_index))
            })
            .filter_map(|validator_index| by_validator.get(&validator_index))
            .map(|entry| Arc::clone(&entry.signed_inclusion_list))
            .collect()
    }

    /// Committee positions worth requesting through `InclusionListsByIndices`: the member has no
    /// stored list and is not an equivocator. Not `!get_inclusion_list_bits`, which also clears the
    /// bits of equivocators, whose lists are never used.
    #[must_use]
    pub fn missing_members(
        &self,
        committee: &InclusionListCommittee<P>,
        slot: Slot,
        dependent_root: H256,
    ) -> Vec<(usize, ValidatorIndex)> {
        let key = (slot, dependent_root);
        let by_validator = self.inclusion_lists.get(&key);
        let equivocators = self.equivocators.get(&key);

        committee
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, validator_index)| {
                !by_validator.is_some_and(|map| map.contains_key(validator_index))
                    && !equivocators.is_some_and(|set| set.contains(validator_index))
            })
            .collect()
    }

    /// [`is_inclusion_list_bits_inclusive`](https://github.com/ethereum/consensus-specs/blob/v1.7.0-beta.1/specs/heze/inclusion-list.md#new-is_inclusion_list_bits_inclusive)
    #[must_use]
    pub fn is_inclusion_list_bits_inclusive(
        &self,
        committee: &InclusionListCommittee<P>,
        slot: Slot,
        dependent_root: H256,
        inclusion_list_bits: &BitVector<P::InclusionListCommitteeSize>,
        only_timely: bool,
    ) -> bool {
        let local = self.get_inclusion_list_bits(committee, slot, dependent_root, only_timely);

        (0..P::InclusionListCommitteeSize::USIZE).all(|position| {
            local.get(position) != Some(true) || inclusion_list_bits.get(position) == Some(true)
        })
    }

    /// Deletes every entry whose slot is below `floor`. The spec's minimum retention, from
    /// [`get_inclusion_list_transactions`](https://github.com/ethereum/consensus-specs/blob/v1.7.0-beta.1/specs/heze/inclusion-list.md#new-get_inclusion_list_transactions):
    ///
    /// > Inclusion lists MUST be retained for at least `MIN_SLOTS_FOR_INCLUSION_LISTS_REQUESTS`
    /// > slots beyond their slot, after which they MAY be pruned.
    pub fn prune_before(&mut self, floor: Slot) {
        self.inclusion_lists.retain(|(slot, _), _| *slot >= floor);
        self.equivocators.retain(|(slot, _), _| *slot >= floor);
    }
}
