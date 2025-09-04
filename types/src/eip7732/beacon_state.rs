use std::sync::Arc;

use derivative::Derivative;
use serde::{Deserialize, Serialize};
use ssz::{BitVector, Hc, Ssz};

use crate::{
    altair::containers::SyncCommittee,
    cache::Cache,
    capella::primitives::WithdrawalIndex,
    collections::{
        Balances, BuilderPendingPayments, BuilderPendingWithdrawals, EpochParticipation, 
        Eth1DataVotes, HistoricalRoots, HistoricalSummaries, InactivityScores, 
        PendingConsolidations, PendingDeposits, PendingPartialWithdrawals, RandaoMixes, 
        RecentRoots, Slashings, Validators,
    },
    deneb::containers::ExecutionPayloadHeader,
    phase0::{
        consts::JustificationBitsLength,
        containers::{BeaconBlockHeader, Checkpoint, Eth1Data, Fork},
        primitives::{DepositIndex, Epoch, Gwei, Slot, UnixSeconds, ValidatorIndex, H256},
    },
    preset::Preset,
};
#[derive(Clone, Debug, Default, Derivative, Deserialize, Serialize, Ssz)]
#[derivative(PartialEq, Eq)]
#[serde(bound = "", deny_unknown_fields)]
pub struct BeaconState<P: Preset> {
    #[serde(with = "serde_utils::string_or_native")]
    pub genesis_time: UnixSeconds,
    pub genesis_validators_root: H256,
    #[serde(with = "serde_utils::string_or_native")]
    pub slot: Slot,
    pub fork: Fork,

    pub latest_block_header: BeaconBlockHeader,
    pub block_roots: RecentRoots<P>,
    pub state_roots: RecentRoots<P>,
    pub historical_roots: HistoricalRoots<P>,

    pub eth1_data: Eth1Data,
    pub eth1_data_votes: Eth1DataVotes<P>,
    #[serde(with = "serde_utils::string_or_native")]
    pub eth1_deposit_index: DepositIndex,

    pub validators: Validators<P>,
    #[serde(with = "serde_utils::string_or_native_sequence")]
    pub balances: Balances<P>,

    pub randao_mixes: RandaoMixes<P>,

    #[serde(with = "serde_utils::string_or_native_sequence")]
    pub slashings: Slashings<P>,

    #[serde(with = "serde_utils::string_or_native_sequence")]
    pub previous_epoch_participation: EpochParticipation<P>,
    #[serde(with = "serde_utils::string_or_native_sequence")]
    pub current_epoch_participation: EpochParticipation<P>,

    pub justification_bits: BitVector<JustificationBitsLength>,
    pub previous_justified_checkpoint: Checkpoint,
    pub current_justified_checkpoint: Checkpoint,
    pub finalized_checkpoint: Checkpoint,

    #[serde(with = "serde_utils::string_or_native_sequence")]
    pub inactivity_scores: InactivityScores<P>,

    pub current_sync_committee: Arc<Hc<SyncCommittee<P>>>,
    pub next_sync_committee: Arc<Hc<SyncCommittee<P>>>,

    pub latest_execution_payload_header: ExecutionPayloadHeader<P>,

    #[serde(with = "serde_utils::string_or_native")]
    pub next_withdrawal_index: WithdrawalIndex,
    #[serde(with = "serde_utils::string_or_native")]
    pub next_withdrawal_validator_index: ValidatorIndex,

    pub historical_summaries: HistoricalSummaries<P>,
    #[serde(with = "serde_utils::string_or_native")]
    pub deposit_requests_start_index: DepositIndex,
    #[serde(with = "serde_utils::string_or_native")]
    pub deposit_balance_to_consume: Gwei,
    #[serde(with = "serde_utils::string_or_native")]
    pub exit_balance_to_consume: Gwei,
    #[serde(with = "serde_utils::string_or_native")]
    pub earliest_exit_epoch: Epoch,
    #[serde(with = "serde_utils::string_or_native")]
    pub consolidation_balance_to_consume: Gwei,
    #[serde(with = "serde_utils::string_or_native")]
    pub earliest_consolidation_epoch: Epoch,
    pub pending_deposits: PendingDeposits<P>,
    pub pending_partial_withdrawals: PendingPartialWithdrawals<P>,
    pub pending_consolidations: PendingConsolidations<P>,

    pub execution_payload_availability: BitVector<P::SlotsPerHistoricalRoot>,
    pub builder_pending_payments: BuilderPendingPayments<P>,
    pub builder_pending_withdrawals: BuilderPendingWithdrawals<P>,
    pub latest_block_hash: H256,
    pub latest_withdrawals_root: H256,

    #[derivative(PartialEq = "ignore")]
    #[serde(skip)]
    #[ssz(skip)]
    pub cache: Cache,
}

impl<P: Preset> BeaconState<P> {
    pub fn get_execution_payload_status(&self, slot_index: usize) -> Option<bool> {
        self.execution_payload_availability.get(slot_index)
    }
}