use core::ops::{Add as _, Index as _, Rem as _};
use std::io::pipe;

use anyhow::{ensure, Result};
use bls::traits::SignatureBytes as _;
use execution_engine::{ExecutionEngine, NullExecutionEngine};
use helper_functions::{
    accessors::{self, get_current_epoch, get_randao_mix},
    electra::{is_fully_withdrawable_validator, is_partially_withdrawable_validator},
    error::SignatureKind,
    misc::{
        self, compute_timestamp_at_slot, get_max_effective_balance,
        kzg_commitment_to_versioned_hash,
    },
    mutators::{balance, decrease_balance},
    predicates::{has_builder_withdrawal_credential, is_active_validator, is_parent_block_full},
    signing::SignForSingleFork as _,
    slot_report::SlotReport,
    verifier::{SingleVerifier, Verifier},
};
use itertools::Itertools as _;
use pubkey_cache::PubkeyCache;
use ssz::{ContiguousList, PersistentList, SszHash as _};
use tap::Pipe as _;
use try_from_iterator::TryFromIterator as _;
use typenum::Unsigned as _;
use types::{
    capella::containers::Withdrawal,
    combined::ExecutionPayloadParams,
    config::Config,
    deneb::containers::ExecutionPayloadHeader,
    gloas::{
        beacon_state::BeaconState,
        containers::{
            BeaconBlock, BeaconBlockBody, BuilderPendingPayment, BuilderPendingWithdrawal,
            ExecutionPayloadBid, SignedBeaconBlock, SignedExecutionPayloadBid,
        },
    },
    phase0::{
        consts::FAR_FUTURE_EPOCH,
        containers::SignedVoluntaryExit,
        primitives::{ExecutionAddress, H256},
    },
    preset::Preset,
    traits::{PostGloasBeaconBlockBody, PostGloasBeaconState},
};

use crate::{
    altair,
    unphased::{self, Error},
};

#[cfg(feature = "metrics")]
use prometheus_metrics::METRICS;

/// [`process_block`](TODO(feature/electra))
///
/// This also serves as a substitute for [`compute_new_state_root`]. `compute_new_state_root` as
/// defined in `consensus-specs` uses `state_transition`, but in practice `state` will already be
/// processed up to `block.slot`, which would make `process_slots` fail due to the restriction added
/// in [version 0.11.3]. `consensus-specs` [originally used `process_block`] but it was [lost].
///
/// [`compute_new_state_root`]:        https://github.com/ethereum/consensus-specs/blob/2ef55744df782eb153fc0a3b1c7875b8c2e11730/specs/phase0/validator.md#state-root
/// [version 0.11.3]:                  https://github.com/ethereum/consensus-specs/releases/tag/v0.11.3
/// [originally used `process_block`]: https://github.com/ethereum/consensus-specs/commit/103a66b2af9d9ec1fd1c70adc8e9029af5775c1c#diff-abbdef70b08ada829d740f06c004c154R298-R301
/// [lost]:                            https://github.com/ethereum/consensus-specs/commit/2dbc33327084d2814958f92eb0a838b9bc161903#diff-e96c612010477fc9536e3ff1ef1a1d5dR343-R346
pub fn process_block<P: Preset>(
    config: &Config,
    pubkey_cache: &PubkeyCache,
    state: &mut BeaconState<P>,
    block: &BeaconBlock<P>,
    mut verifier: impl Verifier,
    slot_report: impl SlotReport,
) -> Result<()> {
    #[cfg(feature = "metrics")]
    let _timer = METRICS
        .get()
        .map(|metrics| metrics.block_transition_times.start_timer());

    verifier.reserve(count_required_signatures(block));

    custom_process_block(
        config,
        pubkey_cache,
        state,
        block,
        NullExecutionEngine,
        &mut verifier,
        slot_report,
    )?;

    verifier.finish()
}

pub fn process_block_for_gossip<P: Preset>(
    config: &Config,
    pubkey_cache: &PubkeyCache,
    state: &BeaconState<P>,
    block: &SignedBeaconBlock<P>,
) -> Result<()> {
    debug_assert_eq!(state.slot, block.message.slot);

    unphased::process_block_header_for_gossip(config, state, &block.message)?;

    let public_key = accessors::public_key(state, block.message.proposer_index)?;

    SingleVerifier.verify_singular(
        block.message.signing_root(config, state),
        block.signature,
        pubkey_cache.get_or_insert(*public_key)?,
        SignatureKind::Block,
    )?;

    Ok(())
}

pub fn count_required_signatures<P: Preset>(block: &BeaconBlock<P>) -> usize {
    altair::count_required_signatures(block) + block.body.bls_to_execution_changes.len()
}

pub fn custom_process_block<P: Preset>(
    config: &Config,
    pubkey_cache: &PubkeyCache,
    state: &mut BeaconState<P>,
    block: &BeaconBlock<P>,
    execution_engine: impl ExecutionEngine<P>,
    mut verifier: impl Verifier,
    slot_report: impl SlotReport,
) -> Result<()> {
    debug_assert_eq!(state.slot, block.slot);

    unphased::process_block_header(config, state, block)?;

    // > [Modified in Gloas:EIP7732]
    process_withdrawals(state)?;

    // > [New in Gloas:EIP7732]
    process_execution_payload_bid(config, pubkey_cache, state, block)?;

    unphased::process_randao(config, pubkey_cache, state, &block.body, &mut verifier)?;
    unphased::process_eth1_data(state, &block.body)?;

    // > [Modified in Gloas:EIP7732]
    // process_operations(
    //     config,
    //     pubkey_cache,
    //     state,
    //     &block.body,
    //     &mut verifier,
    //     &mut slot_report,
    // )?;

    altair::process_sync_aggregate(
        config,
        pubkey_cache,
        state,
        block.body.sync_aggregate,
        verifier,
        slot_report,
    )
}

fn is_builder_payment_withdrawable<P: Preset>(
    state: &(impl PostGloasBeaconState<P> + ?Sized),
    withdrawal: &BuilderPendingWithdrawal,
) -> Result<bool> {
    let builder = state.validators().get(withdrawal.builder_index)?;
    let current_epoch = get_current_epoch(state);

    Ok(builder.withdrawable_epoch >= current_epoch || !builder.slashed)
}

#[expect(clippy::too_many_lines)]
pub fn get_expected_withdrawals<P: Preset>(
    state: &(impl PostGloasBeaconState<P> + ?Sized),
) -> Result<(Vec<Withdrawal>, usize, usize)> {
    let epoch = get_current_epoch(state);
    let total_validators = state.validators().len_u64();
    let max_pending_partials_per_withdrawals_sweep: usize =
        P::MAX_PENDING_PARTIALS_PER_WITHDRAWALS_SWEEP.try_into()?;

    let mut withdrawal_index = state.next_withdrawal_index();
    let mut validator_index = state.next_withdrawal_validator_index();
    let mut withdrawals: Vec<Withdrawal> = vec![];
    let mut processed_partial_withdrawals_count = 0;
    let mut processed_builder_withdrawals_count = 0;

    // > [New in Gloas:EIP7732] Sweep for builder payments
    for withdrawal in &state.builder_pending_withdrawals().clone() {
        if withdrawal.withdrawable_epoch > epoch
            || withdrawals.len() + 1 == P::MaxWithdrawalsPerPayload::USIZE
        {
            break;
        }

        if is_builder_payment_withdrawable(state, withdrawal)? {
            let builder = state.validators().get(withdrawal.builder_index)?;
            let total_withdrawn = withdrawals
                .iter()
                .filter(|w| w.validator_index == withdrawal.builder_index)
                .map(|w| w.amount)
                .sum();
            let balance = state
                .balances()
                .get(withdrawal.builder_index)
                .copied()?
                .saturating_sub(total_withdrawn);

            let withdrawable_balance = if builder.slashed {
                withdrawal.amount.min(balance)
            } else if balance > P::MIN_ACTIVATION_BALANCE {
                withdrawal
                    .amount
                    .min(balance.saturating_sub(P::MIN_ACTIVATION_BALANCE))
            } else {
                0
            };

            withdrawals.push(Withdrawal {
                index: withdrawal_index,
                validator_index: withdrawal.builder_index,
                address: withdrawal.fee_recipient,
                amount: withdrawable_balance,
            });
            withdrawal_index += 1;
        }

        processed_builder_withdrawals_count += 1;
    }

    // > Sweep for pending partial withdrawals
    let bound = withdrawals
        .len()
        .saturating_add(max_pending_partials_per_withdrawals_sweep)
        .min(P::MaxWithdrawalsPerPayload::USIZE - 1);
    for withdrawal in &state.pending_partial_withdrawals().clone() {
        if withdrawal.withdrawable_epoch > epoch || withdrawals.len() == bound {
            break;
        }

        let validator = state.validators().get(withdrawal.validator_index)?;
        let has_sufficient_effective_balance =
            validator.effective_balance >= P::MIN_ACTIVATION_BALANCE;
        let total_withdrawn = withdrawals
            .iter()
            .filter(|w| w.validator_index == withdrawal.validator_index)
            .map(|w| w.amount)
            .sum();
        let validator_balance = state
            .balances()
            .get(withdrawal.validator_index)
            .copied()?
            .saturating_sub(total_withdrawn);
        let has_excess_balance = validator_balance > P::MIN_ACTIVATION_BALANCE;

        if validator.exit_epoch == FAR_FUTURE_EPOCH
            && has_sufficient_effective_balance
            && has_excess_balance
        {
            let withdrawable_balance = withdrawal
                .amount
                .min(validator_balance - P::MIN_ACTIVATION_BALANCE);

            let mut address = ExecutionAddress::zero();

            address.assign_from_slice(&validator.withdrawal_credentials[12..]);

            withdrawals.push(Withdrawal {
                index: withdrawal_index,
                validator_index: withdrawal.validator_index,
                address,
                amount: withdrawable_balance,
            });

            withdrawal_index += 1;
        }

        processed_partial_withdrawals_count += 1;
    }

    // > Sweep for remaining
    for _ in 0..bound {
        let validator = state.validators().get(validator_index)?;

        let partially_withdrawn_balance = withdrawals
            .iter()
            .filter(|withdrawal| withdrawal.validator_index == validator_index)
            .map(|withdrawal| withdrawal.amount)
            .sum();

        let balance = state
            .balances()
            .get(validator_index)
            .copied()?
            .saturating_sub(partially_withdrawn_balance);

        let address = validator
            .withdrawal_credentials
            .as_bytes()
            .index(H256::len_bytes() - ExecutionAddress::len_bytes()..)
            .pipe(ExecutionAddress::from_slice);

        if is_fully_withdrawable_validator(validator, balance, epoch) {
            withdrawals.push(Withdrawal {
                index: withdrawal_index,
                validator_index,
                address,
                amount: balance,
            });

            withdrawal_index = withdrawal_index
                .checked_add(1)
                .ok_or(Error::<P>::WithdrawalIndexOverflow)?;
        } else if is_partially_withdrawable_validator::<P>(validator, balance) {
            withdrawals.push(Withdrawal {
                index: withdrawal_index,
                validator_index,
                address,
                amount: balance
                    .checked_sub(get_max_effective_balance::<P>(validator))
                    .expect(
                        "is_partially_withdrawable_validator should only \
                         return true if the validator has excess balance",
                    ),
            });

            withdrawal_index = withdrawal_index
                .checked_add(1)
                .ok_or(Error::<P>::WithdrawalIndexOverflow)?;
        }

        if withdrawals.len() == P::MaxWithdrawalsPerPayload::USIZE {
            break;
        }

        validator_index = validator_index
            .checked_add(1)
            .ok_or(Error::<P>::ValidatorIndexOverflow)?
            .checked_rem(total_validators)
            .expect("total_validators being 0 should prevent the loop from being executed");
    }

    Ok((
        withdrawals,
        processed_builder_withdrawals_count,
        processed_partial_withdrawals_count,
    ))
}

pub fn process_withdrawals<P: Preset>(state: &mut impl PostGloasBeaconState<P>) -> Result<()> {
    if !is_parent_block_full(state) {
        return Ok(());
    }

    let (withdrawals, processed_builder_withdrawals_count, processed_partial_withdrawals_count) =
        get_expected_withdrawals(state)?;
    let withdrawals_list =
        ContiguousList::<Withdrawal, P::MaxWithdrawalsPerPayload>::try_from_iter(
            withdrawals
                .into_iter()
                .take(P::MaxWithdrawalsPerPayload::USIZE),
        )?;

    *state.latest_withdrawals_root_mut() = withdrawals_list.hash_tree_root();
    for withdrawal in withdrawals_list.as_ref() {
        let Withdrawal {
            amount,
            validator_index,
            ..
        } = withdrawal;

        decrease_balance(balance(state, *validator_index)?, *amount);
    }

    // > Update the pending builder withdrawals
    *state.builder_pending_withdrawals_mut() = PersistentList::try_from_iter(
        state
            .builder_pending_withdrawals()
            .into_iter()
            .take(processed_builder_withdrawals_count)
            .filter(|&withdrawal| {
                !is_builder_payment_withdrawable(state, withdrawal).is_ok_and(|is_true| is_true)
            })
            .cloned()
            .chain(
                state
                    .builder_pending_withdrawals()
                    .into_iter()
                    .copied()
                    .skip(processed_builder_withdrawals_count),
            )
            .take(P::BuilderPendingWithdrawalsLimit::USIZE),
    )?;

    // > Update pending partial withdrawals
    *state.pending_partial_withdrawals_mut() = PersistentList::try_from_iter(
        state
            .pending_partial_withdrawals()
            .into_iter()
            .copied()
            .skip(processed_partial_withdrawals_count),
    )?;

    // > Update the next withdrawal index if this block contained withdrawals
    if let Some(latest_withdrawal) = withdrawals_list.last() {
        *state.next_withdrawal_index_mut() = latest_withdrawal.index + 1;
    }

    // > Update the next validator index to start the next withdrawal sweep
    if withdrawals_list.len() == P::MaxWithdrawalsPerPayload::USIZE {
        // > Next sweep starts after the latest withdrawal's validator index
        let next_validator_index = withdrawals_list
            .last()
            .expect(
                "the NonZero bound on P::MaxWithdrawalsPerPayload \
                 ensures that withdrawals is not empty",
            )
            .validator_index
            .add(1)
            .rem(state.validators().len_u64());

        *state.next_withdrawal_validator_index_mut() = next_validator_index;
    } else {
        // > Advance sweep by the max length of the sweep if there was not a full set of withdrawals
        let next_index =
            state.next_withdrawal_validator_index() + P::MAX_VALIDATORS_PER_WITHDRAWALS_SWEEP;

        let next_validator_index = next_index % state.validators().len_u64();

        *state.next_withdrawal_validator_index_mut() = next_validator_index;
    }

    Ok(())
}

fn validate_execution_payload_bid_signature_with_verifier<P: Preset>(
    config: &Config,
    pubkey_cache: &PubkeyCache,
    state: &impl PostGloasBeaconState<P>,
    signed_bid: SignedExecutionPayloadBid,
    mut verifier: impl Verifier,
) -> Result<()> {
    let SignedExecutionPayloadBid {
        message: execution_payload_bid,
        signature,
    } = signed_bid;
    let builder = state
        .validators()
        .get(execution_payload_bid.builder_index)?;

    // > Verify signature
    verifier.verify_singular(
        execution_payload_bid.signing_root(config, state),
        signature,
        pubkey_cache.get_or_insert(builder.pubkey)?,
        SignatureKind::ExecutionPayloadBid,
    )
}

pub fn process_execution_payload_bid<P: Preset>(
    config: &Config,
    pubkey_cache: &PubkeyCache,
    state: &mut impl PostGloasBeaconState<P>,
    block: &BeaconBlock<P>,
) -> Result<()> {
    let signed_bid = block.body.signed_execution_payload_bid;
    let ExecutionPayloadBid {
        builder_index,
        value: amount,
        slot,
        parent_block_hash,
        parent_block_root,
        fee_recipient,
        ..
    } = signed_bid.message;
    let builder = state.validators().get(builder_index)?;

    // > For self-builds, amount must be zero regardless of withdrawal credential prefix
    if builder_index == block.proposer_index {
        ensure!(amount == 0, Error::<P>::NoneZeroBidValue);
        ensure!(
            signed_bid.signature.is_empty(),
            Error::<P>::ExecutionPayloadBidSignatureInvalid
        );
    } else {
        ensure!(
            has_builder_withdrawal_credential(builder),
            Error::<P>::ExecutionPayloadBidNotBuilder
        );
        ensure!(
            validate_execution_payload_bid_signature_with_verifier(
                config,
                pubkey_cache,
                state,
                signed_bid,
                SingleVerifier,
            )
            .is_ok(),
            Error::<P>::ExecutionPayloadBidSignatureInvalid
        );
    }

    let current_epoch = get_current_epoch(state);
    ensure!(
        is_active_validator(builder, current_epoch),
        Error::<P>::ValidatorNotActive {
            index: builder_index,
            validator: builder.clone(),
            current_epoch
        }
    );
    ensure!(
        !builder.slashed,
        Error::<P>::ValidatorAlreadySlashed {
            index: builder_index,
        }
    );

    // > Check that the builder is active, non-slashed, and has funds to cover the bid
    let builder_balance = *state.balances().get(builder_index)?;
    let pending_withdrawals = state
        .builder_pending_withdrawals()
        .into_iter()
        .filter_map(|withdrawal| {
            (withdrawal.builder_index == builder_index).then_some(withdrawal.amount)
        })
        .sum();
    let pending_payments = state
        .builder_pending_payments()
        .into_iter()
        .filter(|payment| payment.withdrawal.builder_index == builder_index)
        .map(|payment| payment.withdrawal.amount)
        .sum();
    let total_payment_withdraw_amount = amount
        .saturating_add(pending_payments)
        .saturating_add(pending_withdrawals)
        .saturating_add(P::MIN_ACTIVATION_BALANCE);
    ensure!(
        amount == 0 || builder_balance >= total_payment_withdraw_amount,
        Error::<P>::BuilderBalanceNotSufficient {
            balance: builder_balance,
            payments: total_payment_withdraw_amount,
        }
    );

    // > Verify that the bid is for the current slot
    ensure!(
        slot == block.slot,
        Error::<P>::BidSlotMismatch {
            in_bid: slot,
            in_block: block.slot
        }
    );

    // > Verify that the bid is for the right parent block
    ensure!(
        parent_block_hash == state.latest_block_hash(),
        Error::<P>::BidParentBlockHashMismatch {
            in_bid: parent_block_hash,
            in_state: state.latest_block_hash(),
        }
    );
    ensure!(
        parent_block_root == block.parent_root,
        Error::<P>::BidParentBlockRootMismatch {
            in_bid: parent_block_hash,
            in_block: block.parent_root,
        }
    );

    // > Record the pending payment
    let pending_payment = BuilderPendingPayment {
        weight: 0,
        withdrawal: BuilderPendingWithdrawal {
            fee_recipient,
            amount,
            builder_index,
            withdrawable_epoch: 0,
        },
    };
    *state
        .builder_pending_payments_mut()
        .mod_index_mut(P::SlotsPerEpoch::U64.saturating_add(slot % P::SlotsPerEpoch::U64)) =
        pending_payment;

    // > Cache the signed execution payload bid
    *state.latest_execution_payload_bid_mut() = signed_bid.message;

    Ok(())
}

// pub fn process_operations<P: Preset, V: Verifier>(
//     config: &Config,
//     pubkey_cache: &PubkeyCache,
//     state: &mut impl PostGloasBeaconState<P>,
//     body: &impl PostGloasBeaconBlockBody<P>,
//     mut verifier: V,
//     mut slot_report: impl SlotReport,
// ) -> Result<()> {
//     // > [Modified in Electra:EIP6110]
//     // > Disable former deposit mechanism once all prior deposits are processed
//     let eth1_deposit_index_limit = state
//         .eth1_data()
//         .deposit_count
//         .min(state.deposit_requests_start_index());
//
//     let in_block = body.deposits().len().try_into()?;
//
//     if state.eth1_deposit_index() < eth1_deposit_index_limit {
//         let computed =
//             P::MaxDeposits::U64.min(eth1_deposit_index_limit - state.eth1_deposit_index());
//
//         ensure!(
//             computed == in_block,
//             Error::<P>::DepositCountMismatch { computed, in_block },
//         );
//     } else {
//         ensure!(
//             in_block == 0,
//             Error::<P>::DepositCountMismatch {
//                 computed: 0,
//                 in_block
//             },
//         );
//     }
//
//     for proposer_slashing in body.proposer_slashings().iter().copied() {
//         process_proposer_slashing(
//             config,
//             pubkey_cache,
//             state,
//             proposer_slashing,
//             &mut verifier,
//             &mut slot_report,
//         )?;
//     }
//
//     for attester_slashing in body.attester_slashings() {
//         process_attester_slashing(
//             config,
//             pubkey_cache,
//             state,
//             attester_slashing,
//             &mut verifier,
//             &mut slot_report,
//         )?;
//     }
//
//     // TODO: update on https://github.com/ethereum/consensus-specs/blob/v1.5.0-alpha.1/specs/electra/beacon-chain.md#modified-process_attestation
//     // Parallel iteration with Rayon has some overhead, which is most noticeable when the active
//     // thread pool is busy. `ParallelIterator::collect` appears to wait for worker threads to become
//     // available even if the current thread is itself a worker thread. This tends to happen when
//     // verifying signatures for batches of blocks outside the state transition function.
//     // Fortunately, the other validations in `validate_attestation_with_verifier` take a negligible
//     // amount of time, so we can avoid the issue by running them sequentially.
//     if V::IS_NULL {
//         for attestation in body.attestations() {
//             validate_attestation_with_verifier(
//                 config,
//                 pubkey_cache,
//                 state,
//                 attestation,
//                 &mut verifier,
//             )?;
//         }
//     } else {
//         initialize_shuffled_indices(state, body.attestations().iter())?;
//
//         let triples = helper_functions::par_iter!(body.attestations())
//             .map(|attestation| {
//                 let mut triple = Triple::default();
//
//                 validate_attestation_with_verifier(
//                     config,
//                     pubkey_cache,
//                     state,
//                     attestation,
//                     &mut triple,
//                 )?;
//
//                 Ok(triple)
//             })
//             .collect::<Result<Vec<_>>>()?;
//
//         verifier.extend(triples, SignatureKind::Attestation)?;
//     }
//
//     for attestation in body.attestations() {
//         apply_attestation(config, state, attestation, &mut slot_report)?;
//     }
//
//     // The conditional is not needed for correctness.
//     // It only serves to avoid overhead when processing blocks with no deposits.
//     if !body.deposits().is_empty() {
//         let combined_deposits = unphased::validate_deposits(
//             config,
//             pubkey_cache,
//             state,
//             body.deposits().iter().copied(),
//         )?;
//
//         let deposit_count = body.deposits().len();
//
//         // > Deposits must be processed in order
//         *state.eth1_deposit_index_mut() += DepositIndex::try_from(deposit_count)?;
//
//         apply_deposits(state, combined_deposits, slot_report)?;
//     }
//
//     for voluntary_exit in body.voluntary_exits().iter().copied() {
//         process_voluntary_exit(config, pubkey_cache, state, voluntary_exit, &mut verifier)?;
//     }
//
//     for bls_to_execution_change in body.bls_to_execution_changes().iter().copied() {
//         capella::process_bls_to_execution_change(
//             config,
//             pubkey_cache,
//             state,
//             bls_to_execution_change,
//             &mut verifier,
//         )?;
//     }
//
//     Ok(())
// }

// TODO(gloas): implement [`process_execution_payload_bid`](https://github.com/ethereum/consensus-specs/blob/master/specs/gloas/beacon-chain.md#new-process_execution_payload_bid)
// fn process_execution_payload_for_gossip<P: Preset>(
//     config: &Config,
//     state: &BeaconState<P>,
//     body: &BeaconBlockBody<P>,
// ) -> Result<()> {
//     let payload = &body.execution_payload;
//
//     // > Verify timestamp
//     let computed = compute_timestamp_at_slot(config, state, state.slot);
//     let in_block = payload.timestamp;
//
//     ensure!(
//         computed == in_block,
//         Error::<P>::ExecutionPayloadTimestampMismatch { computed, in_block },
//     );
//
//     // > [Modified in Fulu:EIP7594] Verify commitments are under limit
//     // > [Modified in Fulu:EIP7892] BPO blob schedule
//     let maximum = config
//         .get_blob_schedule_entry(get_current_epoch(state))
//         .max_blobs_per_block;
//     let in_block = body.blob_kzg_commitments.len();
//
//     ensure!(
//         in_block <= maximum,
//         Error::<P>::TooManyBlockKzgCommitments { in_block, maximum },
//     );
//
//     Ok(())
// }
//
// // TODO(gloas): [spec](https://github.com/ethereum/consensus-specs/blob/master/specs/gloas/beacon-chain.md#new-process_execution_payload)
// // Move to `execution_payload_processing.rs`
// fn process_execution_payload<P: Preset>(
//     config: &Config,
//     state: &mut BeaconState<P>,
//     block_root: H256,
//     body: &BeaconBlockBody<P>,
//     execution_engine: impl ExecutionEngine<P>,
// ) -> Result<()> {
//     let payload = &body.execution_payload;
//     let execution_requests = &body.execution_requests;
//
//     // > Verify consistency of the parent hash with respect to the previous execution payload header
//     let in_state = state.latest_execution_payload_header.block_hash;
//     let in_block = payload.parent_hash;
//
//     ensure!(
//         in_state == in_block,
//         Error::<P>::ExecutionPayloadParentHashMismatch { in_state, in_block },
//     );
//
//     // > Verify prev_randao
//     let in_state = get_randao_mix(state, get_current_epoch(state));
//     let in_block = payload.prev_randao;
//
//     ensure!(
//         in_state == in_block,
//         Error::<P>::ExecutionPayloadPrevRandaoMismatch { in_state, in_block },
//     );
//
//     process_execution_payload_for_gossip(config, state, body)?;
//
//     // > Verify the execution payload is valid
//     let versioned_hashes = body
//         .blob_kzg_commitments
//         .iter()
//         .copied()
//         .map(kzg_commitment_to_versioned_hash)
//         .collect();
//
//     execution_engine.notify_new_payload(
//         block_root,
//         payload.clone().into(),
//         Some(ExecutionPayloadParams::Electra {
//             versioned_hashes,
//             parent_beacon_block_root: state.latest_block_header.parent_root,
//             execution_requests: execution_requests.clone(),
//         }),
//         None,
//     )?;
//
//     // > Cache execution payload header
//     state.latest_execution_payload_header = ExecutionPayloadHeader::from(payload);
//
//     Ok(())
// }

pub fn validate_voluntary_exit<P: Preset>(
    config: &Config,
    pubkey_cache: &PubkeyCache,
    state: &BeaconState<P>, // TODO(gloas): change to impl PostGloasBeaconState
    signed_voluntary_exit: SignedVoluntaryExit,
) -> Result<()> {
    validate_voluntary_exit_with_verifier(
        config,
        pubkey_cache,
        state,
        signed_voluntary_exit,
        SingleVerifier,
    )
}

pub fn validate_voluntary_exit_with_verifier<P: Preset>(
    config: &Config,
    pubkey_cache: &PubkeyCache,
    state: &BeaconState<P>, // TODO(gloas): change to impl PostGloasBeaconState
    signed_voluntary_exit: SignedVoluntaryExit,
    verifier: impl Verifier,
) -> Result<()> {
    unphased::validate_voluntary_exit_with_verifier(
        config,
        pubkey_cache,
        state,
        signed_voluntary_exit,
        verifier,
    )?;

    // TODO(gloas): implement `get_pending_balance_to_withdraw` for post-Gloas in accessors
    // [spec](https://github.com/ethereum/consensus-specs/blob/master/specs/gloas/beacon-chain.md#modified-get_pending_balance_to_withdraw)
    // > [Modified in Gloas:EIP7732] To account for pending builder payments
    // ensure!(
    //     get_pending_balance_to_withdraw(state, signed_voluntary_exit.message.validator_index) == 0,
    //     Error::<P>::VoluntaryExitWithPendingWithdrawals,
    // );

    Ok(())
}

#[cfg(test)]
mod spec_tests {
    use core::fmt::Debug;

    use execution_engine::MockExecutionEngine;
    use helper_functions::{
        slot_report::NullSlotReport,
        verifier::{NullVerifier, SingleVerifier},
    };
    use serde::Deserialize;
    use spec_test_utils::{BlsSetting, Case};
    use ssz::SszReadDefault;
    use test_generator::test_resources;
    use types::{
        deneb::containers::ExecutionPayload,
        electra::containers::{Attestation, AttesterSlashing},
        phase0::containers::Deposit,
        preset::{Mainnet, Minimal},
        traits::BeaconState as _,
    };

    use super::*;

    use crate::{capella, electra};

    // We only honor `bls_setting` in `Attestation` tests. They are the only ones that set it to 2.

    #[derive(Deserialize)]
    struct Execution {
        execution_valid: bool,
    }

    macro_rules! processing_tests {
        (
            $module_name: ident,
            $processing_function: expr,
            $operation_name: literal,
            $mainnet_glob: literal,
            $minimal_glob: literal,
        ) => {
            mod $module_name {
                use super::*;

                #[test_resources($mainnet_glob)]
                fn mainnet(case: Case) {
                    run_processing_case_specialized::<Mainnet>(case);
                }

                #[test_resources($minimal_glob)]
                fn minimal(case: Case) {
                    run_processing_case_specialized::<Minimal>(case);
                }

                fn run_processing_case_specialized<P: Preset>(case: Case) {
                    run_processing_case::<P, _>(case, $operation_name, $processing_function);
                }
            }
        };
    }

    macro_rules! validation_tests {
        (
            $module_name: ident,
            $validation_function: expr,
            $operation_name: literal,
            $mainnet_glob: literal,
            $minimal_glob: literal,
        ) => {
            mod $module_name {
                use super::*;

                #[test_resources($mainnet_glob)]
                fn mainnet(case: Case) {
                    run_validation_case_specialized::<Mainnet>(case);
                }

                #[test_resources($minimal_glob)]
                fn minimal(case: Case) {
                    run_validation_case_specialized::<Minimal>(case);
                }

                fn run_validation_case_specialized<P: Preset>(case: Case) {
                    run_validation_case::<P, _, _>(case, $operation_name, $validation_function);
                }
            }
        };
    }

    // Test files for `process_block_header` are named `block.*` and contain `BeaconBlock`s.
    processing_tests! {
        process_block_header,
        |config, _, state, block: BeaconBlock<_>, _| unphased::process_block_header(config, state, &block),
        "block",
        "consensus-spec-tests/tests/mainnet/gloas/operations/block_header/*/*",
        "consensus-spec-tests/tests/minimal/gloas/operations/block_header/*/*",
    }

    // TODO(gloas): update `state` param to be compatible with GloasBeaconState
    // processing_tests! {
    //     process_consolidation_request,
    //     |config, _, state, consolidation_request, _| electra::process_consolidation_request(config, state, consolidation_request),
    //     "consolidation_request",
    //     "consensus-spec-tests/tests/mainnet/gloas/operations/consolidation_request/*/*",
    //     "consensus-spec-tests/tests/minimal/gloas/operations/consolidation_request/*/*",
    // }

    // TODO(gloas): update `state` param to be compatible with GloasBeaconState
    // processing_tests! {
    //     process_proposer_slashing,
    //     |config, pubkey_cache, state, proposer_slashing, _| {
    //         electra::process_proposer_slashing(
    //             config,
    //             pubkey_cache,
    //             state,
    //             proposer_slashing,
    //             SingleVerifier,
    //             NullSlotReport,
    //         )
    //     },
    //     "proposer_slashing",
    //     "consensus-spec-tests/tests/mainnet/gloas/operations/proposer_slashing/*/*",
    //     "consensus-spec-tests/tests/minimal/gloas/operations/proposer_slashing/*/*",
    // }

    // TODO(gloas): update `state` param to be compatible with GloasBeaconState
    // processing_tests! {
    //     process_attester_slashing,
    //     |config, pubkey_cache, state, attester_slashing: AttesterSlashing<P>, _| {
    //         electra::process_attester_slashing(
    //             config,
    //             pubkey_cache,
    //             state,
    //             &attester_slashing,
    //             SingleVerifier,
    //             NullSlotReport,
    //         )
    //     },
    //     "attester_slashing",
    //     "consensus-spec-tests/tests/mainnet/gloas/operations/attester_slashing/*/*",
    //     "consensus-spec-tests/tests/minimal/gloas/operations/attester_slashing/*/*",
    // }

    // TODO(gloas): uncomment once [`process_attestation`](https://github.com/ethereum/consensus-specs/blob/v1.6.0-beta.0/specs/gloas/beacon-chain.md#modified-process_attestation) implemented
    // processing_tests! {
    //     process_attestation,
    //     |config, pubkey_cache, state, attestation, bls_setting| {
    //         process_attestation(
    //             config,
    //             pubkey_cache,
    //             state,
    //             &attestation,
    //             bls_setting,
    //         )
    //     },
    //     "attestation",
    //     "consensus-spec-tests/tests/mainnet/gloas/operations/attestation/*/*",
    //     "consensus-spec-tests/tests/minimal/gloas/operations/attestation/*/*",
    // }

    // TODO(gloas): update `state` param to be compatible with GloasBeaconState
    // processing_tests! {
    //     process_bls_to_execution_change,
    //     |config, pubkey_cache, state, bls_to_execution_change, _| {
    //         capella::process_bls_to_execution_change(
    //             config,
    //             pubkey_cache,
    //             state,
    //             bls_to_execution_change,
    //             SingleVerifier,
    //         )
    //     },
    //     "address_change",
    //     "consensus-spec-tests/tests/mainnet/gloas/operations/bls_to_execution_change/*/*",
    //     "consensus-spec-tests/tests/minimal/gloas/operations/bls_to_execution_change/*/*",
    // }

    // TODO(gloas): update `state` param to be compatible with GloasBeaconState
    // processing_tests! {
    //     process_deposit,
    //     |config, pubkey_cache, state, deposit, _| process_deposit(config, pubkey_cache, state, deposit),
    //     "deposit",
    //     "consensus-spec-tests/tests/mainnet/gloas/operations/deposit/*/*",
    //     "consensus-spec-tests/tests/minimal/gloas/operations/deposit/*/*",
    // }
    //
    // `process_deposit_data` reimplements deposit validation differently for performance reasons,
    // so we need to test it separately.
    // processing_tests! {
    //     process_deposit_data,
    //     |config, pubkey_cache, state, deposit, _| {
    //         unphased::verify_deposit_merkle_branch(state, state.eth1_deposit_index, deposit)?;
    //         electra::process_deposit_data(config, pubkey_cache, state, deposit.data)?;
    //         Ok(())
    //     },
    //     "deposit",
    //     "consensus-spec-tests/tests/mainnet/gloas/operations/deposit/*/*",
    //     "consensus-spec-tests/tests/minimal/gloas/operations/deposit/*/*",
    // }

    // TODO(gloas): update `state` param to be compatible with GloasBeaconState
    // processing_tests! {
    //     process_voluntary_exit,
    //     |config, pubkey_cache, state, voluntary_exit, _| {
    //         electra::process_voluntary_exit(
    //             config,
    //             pubkey_cache,
    //             state,
    //             voluntary_exit,
    //             SingleVerifier,
    //         )
    //     },
    //     "voluntary_exit",
    //     "consensus-spec-tests/tests/mainnet/gloas/operations/voluntary_exit/*/*",
    //     "consensus-spec-tests/tests/minimal/gloas/operations/voluntary_exit/*/*",
    // }

    processing_tests! {
        process_sync_aggregate,
        |config, pubkey_cache, state, sync_aggregate, _| {
            altair::process_sync_aggregate(
                config,
                pubkey_cache,
                state,
                sync_aggregate,
                SingleVerifier,
                NullSlotReport,
            )
        },
        "sync_aggregate",
        "consensus-spec-tests/tests/mainnet/gloas/operations/sync_aggregate/*/*",
        "consensus-spec-tests/tests/minimal/gloas/operations/sync_aggregate/*/*",
    }

    // TODO(gloas): update `state` param to be compatible with GloasBeaconState
    // processing_tests! {
    //     process_deposit_request,
    //     |_, _, state, deposit_request, _| electra::process_deposit_request(state, deposit_request),
    //     "deposit_request",
    //     "consensus-spec-tests/tests/mainnet/gloas/operations/deposit_request/*/*",
    //     "consensus-spec-tests/tests/minimal/gloas/operations/deposit_request/*/*",
    // }

    // TODO(gloas): update `state` param to be compatible with GloasBeaconState
    // processing_tests! {
    //     process_withdrawal_request,
    //     |config, _, state, withdrawal_request, _| electra::process_withdrawal_request(config, state, withdrawal_request),
    //     "withdrawal_request",
    //     "consensus-spec-tests/tests/mainnet/gloas/operations/withdrawal_request/*/*",
    //     "consensus-spec-tests/tests/minimal/gloas/operations/withdrawal_request/*/*",
    // }

    validation_tests! {
        validate_proposer_slashing,
        |config, pubkey_cache, state, proposer_slashing| {
            unphased::validate_proposer_slashing(config, pubkey_cache, state, proposer_slashing)
        },
        "proposer_slashing",
        "consensus-spec-tests/tests/mainnet/gloas/operations/proposer_slashing/*/*",
        "consensus-spec-tests/tests/minimal/gloas/operations/proposer_slashing/*/*",
    }

    validation_tests! {
        validate_attester_slashing,
        |config, pubkey_cache, state, attester_slashing: AttesterSlashing<P>| {
            unphased::validate_attester_slashing(config, pubkey_cache, state, &attester_slashing)
        },
        "attester_slashing",
        "consensus-spec-tests/tests/mainnet/gloas/operations/attester_slashing/*/*",
        "consensus-spec-tests/tests/minimal/gloas/operations/attester_slashing/*/*",
    }

    // TODO(gloas): update `state` param to be compatible with GloasBeaconState
    // validation_tests! {
    //     validate_voluntary_exit,
    //     |config, pubkey_cache, state, voluntary_exit| {
    //         electra::validate_voluntary_exit_with_verifier(config, pubkey_cache, state, voluntary_exit, SingleVerifier)
    //     },
    //     "voluntary_exit",
    //     "consensus-spec-tests/tests/mainnet/gloas/operations/voluntary_exit/*/*",
    //     "consensus-spec-tests/tests/minimal/gloas/operations/voluntary_exit/*/*",
    // }

    // TODO(gloas): update `state` param to be compatible with GloasBeaconState
    // TODO(feature/electra): comment this & run missing test script
    // validation_tests! {
    //     validate_bls_to_execution_change,
    //     |config, pubkey_cache, state, bls_to_execution_change| {
    //         capella::validate_bls_to_execution_change(config, pubkey_cache, state, bls_to_execution_change)
    //     },
    //     "address_change",
    //     "consensus-spec-tests/tests/mainnet/gloas/operations/bls_to_execution_change/*/*",
    //     "consensus-spec-tests/tests/minimal/gloas/operations/bls_to_execution_change/*/*",
    // }

    // TODO(gloas): uncomment after `process_execution_payload` implemented
    // #[test_resources("consensus-spec-tests/tests/mainnet/gloas/operations/execution_payload/*/*")]
    // fn mainnet_execution_payload(case: Case) {
    //     run_execution_payload_case::<Mainnet>(case);
    // }
    //
    // #[test_resources("consensus-spec-tests/tests/minimal/gloas/operations/execution_payload/*/*")]
    // fn minimal_execution_payload(case: Case) {
    //     run_execution_payload_case::<Minimal>(case);
    // }

    // TODO(gloas): update `state` param to be compatible with GloasBeaconState
    // #[test_resources("consensus-spec-tests/tests/mainnet/gloas/operations/withdrawals/*/*")]
    // fn mainnet_withdrawals(case: Case) {
    //     run_withdrawals_case::<Mainnet>(case);
    // }
    //
    // #[test_resources("consensus-spec-tests/tests/minimal/gloas/operations/withdrawals/*/*")]
    // fn minimal_withdrawals(case: Case) {
    //     run_withdrawals_case::<Minimal>(case);
    // }

    fn run_processing_case<P: Preset, O: SszReadDefault>(
        case: Case,
        operation_name: &str,
        processing_function: impl FnOnce(
            &Config,
            &PubkeyCache,
            &mut BeaconState<P>,
            O,
            BlsSetting,
        ) -> Result<()>,
    ) {
        let pubkey_cache = PubkeyCache::default();
        let mut state = case.ssz_default("pre");
        let operation = case.ssz_default(operation_name);
        let post_option = case.try_ssz_default("post");
        let bls_setting = case.meta().bls_setting;

        let result = processing_function(
            &P::default_config(),
            &pubkey_cache,
            &mut state,
            operation,
            bls_setting,
        )
        .map(|()| state);

        if let Some(expected_post) = post_option {
            let actual_post = result.expect("operation processing should succeed");
            assert_eq!(actual_post, expected_post);
        } else {
            result.expect_err("operation processing should fail");
        }
    }

    fn run_validation_case<P: Preset, O: SszReadDefault, R: Debug>(
        case: Case,
        operation_name: &str,
        validation_function: impl FnOnce(&Config, &PubkeyCache, &mut BeaconState<P>, O) -> Result<R>,
    ) {
        let pubkey_cache = PubkeyCache::default();
        let mut state = case.ssz_default("pre");
        let operation = case.ssz_default(operation_name);
        let post_exists = case.exists("post");

        let result =
            validation_function(&P::default_config(), &pubkey_cache, &mut state, operation);

        if post_exists {
            result.expect("validation should succeed");
        } else {
            result.expect_err("validation should fail");
        }
    }

    // TODO(gloas): uncomment after `process_execution_payload` implemented
    // fn run_execution_payload_case<P: Preset>(case: Case) {
    //     let mut state = case.ssz_default::<BeaconState<P>>("pre");
    //     let body = case.ssz_default("body");
    //     let post_option = case.try_ssz_default("post");
    //     let Execution { execution_valid } = case.yaml("execution");
    //     let execution_engine = MockExecutionEngine::new(execution_valid, false, None);
    //
    //     let result = process_execution_payload(
    //         &P::default_config(),
    //         &mut state,
    //         H256::default(),
    //         &body,
    //         &execution_engine,
    //     )
    //     .map(|()| state);
    //
    //     if let Some(expected_post) = post_option {
    //         let actual_post = result.expect("execution payload processing should succeed");
    //         assert_eq!(actual_post, expected_post);
    //     } else {
    //         result.expect_err("execution payload processing should fail");
    //     }
    // }

    // TODO(gloas): uncomment after `state` param is compatible with GloasBeaconState
    // fn run_withdrawals_case<P: Preset>(case: Case) {
    //     let mut state = case.ssz_default::<BeaconState<P>>("pre");
    //     let payload = case.ssz_default::<ExecutionPayload<P>>("execution_payload");
    //     let post_option = case.try_ssz_default("post");
    //
    //     let result = electra::process_withdrawals(&mut state, &payload).map(|()| state);
    //
    //     if let Some(expected_post) = post_option {
    //         let actual_post = result.expect("withdrawals processing should succeed");
    //         assert_eq!(actual_post, expected_post);
    //     } else {
    //         result.expect_err("withdrawals processing should fail");
    //     }
    // }

    // TODO(gloas): uncomment after `state` param is compatible with GloasBeaconState
    // fn process_attestation<P: Preset>(
    //     config: &Config,
    //     pubkey_cache: &PubkeyCache,
    //     state: &mut BeaconState<P>,
    //     attestation: &Attestation<P>,
    //     bls_setting: BlsSetting,
    // ) -> Result<()> {
    //     match bls_setting {
    //         BlsSetting::Optional | BlsSetting::Required => {
    //             electra::validate_attestation_with_verifier(
    //                 config,
    //                 pubkey_cache,
    //                 state,
    //                 attestation,
    //                 SingleVerifier,
    //             )?
    //         }
    //         BlsSetting::Ignored => electra::validate_attestation_with_verifier(
    //             config,
    //             pubkey_cache,
    //             state,
    //             attestation,
    //             NullVerifier,
    //         )?,
    //     }
    //
    //     electra::apply_attestation(config, state, attestation, NullSlotReport)
    // }

    // TODO(gloas): uncomment after `state` param is compatible with GloasBeaconState
    // fn process_deposit<P: Preset>(
    //     config: &Config,
    //     pubkey_cache: &PubkeyCache,
    //     state: &mut BeaconState<P>,
    //     deposit: Deposit,
    // ) -> Result<()> {
    //     let combined_deposits =
    //         unphased::validate_deposits(config, pubkey_cache, state, core::iter::once(deposit))?;
    //
    //     // > Deposits must be processed in order
    //     *state.eth1_deposit_index_mut() += 1;
    //
    //     electra::apply_deposits(state, combined_deposits, NullSlotReport)
    // }
}
