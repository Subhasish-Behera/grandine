//! Received inclusion lists.
//!
//! Model: `(slot, dependent_root) -> validator_index -> entry` (the list and whether it arrived
//! timely), plus the equivocators per key. Equivocators are left out of every read.
//!
//! It is a field of `Store`. Entries below the finalized slot are pruned when finalization
//! advances.
//!
//! In the spec this is the single global `InclusionListStore`; here it lives in `Store`.
#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use ssz::SszHash as _;
use types::{
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
