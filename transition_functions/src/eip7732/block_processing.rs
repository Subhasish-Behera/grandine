use anyhow::{ensure, Result};
use execution_engine::{ExecutionEngine, NullExecutionEngine};
use bit_field::BitField;
use typenum::Unsigned;
use try_from_iterator::TryFromIterator;
use helper_functions::{
    accessors::{self,
        get_attestation_participation_flags, get_base_reward, get_base_reward_per_increment,
        get_beacon_proposer_index, get_current_epoch, get_randao_mix,
        is_attestation_same_slot, attestation_epoch,
    },
    eip7732::{get_indexed_payload_attestation, is_valid_indexed_payload_attestation},
    electra::{get_attesting_indices, is_fully_withdrawable_validator, is_partially_withdrawable_validator},
    error::SignatureKind,
    misc::{compute_epoch_at_slot, compute_timestamp_at_slot, get_max_effective_balance, kzg_commitment_to_versioned_hash},
    mutators::{balance, decrease_balance, increase_balance, compute_exit_epoch_and_update_churn},
    predicates::{
        has_builder_withdrawal_credential, is_active_validator,
    },
    signing::SignForSingleFork as _,
    slot_report::SlotReport,
    verifier::{SingleVerifier, Verifier},
};
use types::phase0::primitives::DepositIndex;
use pubkey_cache::PubkeyCache;
use ssz::SszHash as _;
use types::{
    altair::consts::{PARTICIPATION_FLAG_WEIGHTS, PROPOSER_WEIGHT, WEIGHT_DENOMINATOR},
    capella::containers::Withdrawal,
    combined::Attestation,
    config::Config,
    eip7732::{
        containers::{
            BeaconBlock, BeaconBlockBody, ExecutionPayloadHeader, SignedBeaconBlock,
            SignedExecutionPayloadHeader, PayloadAttestation, PayloadAttestationData,
            IndexedPayloadAttestation, ExecutionPayloadEnvelope, SignedExecutionPayloadEnvelope,
            BuilderPendingWithdrawal, BuilderPendingPayment,
        },
        beacon_state::BeaconState,
    },
    nonstandard::AttestationEpoch,
    phase0::{
        consts::FAR_FUTURE_EPOCH,
        primitives::{Epoch, ExecutionAddress, Gwei, H256},
    },
    preset::Preset,
    traits::{PostCapellaBeaconBlockBody, PostCapellaBeaconState, PostCapellaExecutionPayload, PostEip7732BeaconState},
};

use crate::{
    altair, bellatrix, capella, electra,
    unphased::{self, Error},
};

#[cfg(feature = "metrics")]
use prometheus_metrics::METRICS;

/// Process block for EIP-7732 following Grandine's pattern
pub fn process_block<P: Preset>(
    config: &Config,
    pubkey_cache: &PubkeyCache,
    state: &mut impl PostEip7732BeaconState<P>,
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

/// Process block for gossip validation
pub fn process_block_for_gossip<P: Preset>(
    config: &Config,
    pubkey_cache: &PubkeyCache,
    state: &impl PostEip7732BeaconState<P>,
    block: &SignedBeaconBlock<P>,
) -> Result<()> {
    debug_assert_eq!(state.slot(), block.message.slot);

    unphased::process_block_header_for_gossip(config, state, &block.message)?;

    process_execution_payload_header_for_gossip(config, state, &block.message.body)?;

    let public_key = accessors::public_key(state, block.message.proposer_index)?;

    SingleVerifier.verify_singular(
        block.message.slot.signing_root(config, state),
        block.signature,
        pubkey_cache.get_or_insert(*public_key)?,
        SignatureKind::Block,
    )?;

    Ok(())
}

pub fn count_required_signatures<P: Preset>(block: &BeaconBlock<P>) -> usize {
    altair::count_required_signatures(block) + block.body.bls_to_execution_changes.len() + 1
}

pub fn custom_process_block<P: Preset>(
    config: &Config,
    pubkey_cache: &PubkeyCache,
    state: &mut impl PostEip7732BeaconState<P>,
    block: &BeaconBlock<P>,
    _execution_engine: impl ExecutionEngine<P>,
    verifier: &mut impl Verifier,
    mut slot_report: impl SlotReport,
) -> Result<()> {
    debug_assert_eq!(state.slot(), block.slot);

    unphased::process_block_header(config, state, block)?;
    
    process_withdrawals(state)?;
    
    process_execution_payload_header(config, pubkey_cache, state, block, &mut *verifier)?;
    
    unphased::process_randao(config, pubkey_cache, state, &block.body, &mut *verifier)?;
    
    unphased::process_eth1_data(state, &block.body)?;
    
    process_operations(config, pubkey_cache, state, &block.body, &mut *verifier, &mut slot_report)?;
    
    
    altair::process_sync_aggregate(
        config,
        pubkey_cache,
        state,
        block.body.sync_aggregate,
        verifier,
        slot_report,
    )
}

/// Process execution payload header for gossip validation
pub fn process_execution_payload_header_for_gossip<P: Preset>(
    _config: &Config,
    state: &impl PostEip7732BeaconState<P>,
    body: &BeaconBlockBody<P>,
) -> Result<()> {
    let header = &body.signed_execution_payload_header.message;
    
    // Verify slot
    ensure!(
        header.slot == state.slot(),
        Error::<P>::SlotMismatch { state_slot: state.slot(), block_slot: header.slot }
    );
    
    // Verify parent block hash and root
    ensure!(
        header.parent_block_hash == state.latest_block_hash(),
        Error::<P>::ParentBlockHashMismatch
    );
    
    Ok(())
}

/// Check if builder payment is withdrawable
pub fn is_builder_payment_withdrawable<P: Preset>(
    state: &impl PostEip7732BeaconState<P> ,
    withdrawal: &BuilderPendingWithdrawal,
) -> Result<bool> {
    let builder = state.validators().get(withdrawal.builder_index)?;
    let current_epoch = get_current_epoch(state);
    
    Ok(builder.withdrawable_epoch <= current_epoch || !builder.slashed)
}

pub fn get_expected_withdrawals<P: Preset>(
    state: &impl PostEip7732BeaconState<P>,
) -> Result<(Vec<Withdrawal>, usize, usize)> {
    let epoch = get_current_epoch(state);
    let mut withdrawal_index = state.next_withdrawal_index();
    let mut validator_index = state.next_withdrawal_validator_index();
    let mut withdrawals: Vec<Withdrawal> = Vec::new();
    let mut processed_partial_withdrawals_count = 0;
    let mut processed_builder_withdrawals_count = 0;
    
    for withdrawal in &state.builder_pending_withdrawals().clone() {
        if withdrawal.withdrawable_epoch > epoch 
            || withdrawals.len() + 1 == 16 {
            break;
        }
        
        if is_builder_payment_withdrawable(state, &withdrawal)? {
            let total_withdrawn: Gwei = withdrawals.iter()
                .filter(|w| w.validator_index == withdrawal.builder_index)
                .map(|w| w.amount)
                .sum();
            
            let balance = state.balances()
                .get(withdrawal.builder_index)
                .map_err(|_| anyhow::anyhow!("Invalid builder index"))? - total_withdrawn;
            let builder = state.validators()
                .get(withdrawal.builder_index)
                .map_err(|_| anyhow::anyhow!("Invalid builder index"))?;
            
            let withdrawable_balance = if builder.slashed {
                balance.min(withdrawal.amount)
            } else if balance > P::MIN_ACTIVATION_BALANCE {
                (balance - P::MIN_ACTIVATION_BALANCE).min(withdrawal.amount)
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
    
    let max_pending_partials = P::MAX_PENDING_PARTIALS_PER_WITHDRAWALS_SWEEP as usize;
    let bound = (withdrawals.len() + max_pending_partials)
        .min(16);
    
    for withdrawal in &state.pending_partial_withdrawals().clone() {
        if withdrawal.withdrawable_epoch > epoch || withdrawals.len() == bound {
            break;
        }
        
        let validator = state.validators()
            .get(withdrawal.validator_index)
            .map_err(|_| anyhow::anyhow!("Invalid validator index"))?;
        let has_sufficient_effective_balance = validator.effective_balance >= P::MIN_ACTIVATION_BALANCE;
        
        let total_withdrawn: Gwei = withdrawals.iter()
            .filter(|w| w.validator_index == withdrawal.validator_index)
            .map(|w| w.amount)
            .sum();
        
        let balance = state.balances()
            .get(withdrawal.validator_index)
            .map_err(|_| anyhow::anyhow!("Invalid validator index"))? - total_withdrawn;
        let has_excess_balance = balance > P::MIN_ACTIVATION_BALANCE;
        
        if validator.exit_epoch == FAR_FUTURE_EPOCH
            && has_sufficient_effective_balance
            && has_excess_balance {
            
            let withdrawable_balance = (balance - P::MIN_ACTIVATION_BALANCE).min(withdrawal.amount);
            
            withdrawals.push(Withdrawal {
                index: withdrawal_index,
                validator_index: withdrawal.validator_index,
                address: {
                    let mut address = ExecutionAddress::zero();
                    address.assign_from_slice(&validator.withdrawal_credentials[12..]);
                    address
                },
                amount: withdrawable_balance,
            });
            
            withdrawal_index += 1;
        }
        
        processed_partial_withdrawals_count += 1;
    }
    
    let validators_count = state.validators().len_u64();
    let bound = validators_count.min(P::MAX_VALIDATORS_PER_WITHDRAWALS_SWEEP) as usize;
    
    for _ in 0..bound {
        let validator = state.validators()
            .get(validator_index)
            .map_err(|_| anyhow::anyhow!("Invalid validator index"))?;
        
        let total_withdrawn: Gwei = withdrawals.iter()
            .filter(|w| w.validator_index == validator_index)
            .map(|w| w.amount)
            .sum();
        
        let balance = state.balances()
            .get(validator_index)
            .map_err(|_| anyhow::anyhow!("Invalid validator index"))? - total_withdrawn;
        
        if is_fully_withdrawable_validator(validator, balance, epoch) {
            withdrawals.push(Withdrawal {
                index: withdrawal_index,
                validator_index,
                address: {
                    let mut address = ExecutionAddress::zero();
                    address.assign_from_slice(&validator.withdrawal_credentials[12..]);
                    address
                },
                amount: balance,
            });
            withdrawal_index += 1;
        } else if is_partially_withdrawable_validator::<P>(validator, balance) {
            let max_effective_balance = get_max_effective_balance::<P>(validator);
            withdrawals.push(Withdrawal {
                index: withdrawal_index,
                validator_index,
                address: {
                    let mut address = ExecutionAddress::zero();
                    address.assign_from_slice(&validator.withdrawal_credentials[12..]);
                    address
                },
                amount: balance - max_effective_balance,
            });
            withdrawal_index += 1;
        }
        
        if withdrawals.len() == 16 {
            break;
        }
        
        validator_index = (validator_index + 1) % validators_count;
    }
    
    Ok((withdrawals, processed_builder_withdrawals_count, processed_partial_withdrawals_count))
}

fn process_withdrawals_common<P: Preset>(
    state: &mut impl PostEip7732BeaconState<P>,
    expected_withdrawals: Vec<Withdrawal>,
    _builder_withdrawals_count: Option<usize>,
    partial_withdrawals_count: Option<usize>,
) -> Result<()> {
    if let Some(partial_withdrawals_count) = partial_withdrawals_count {
        let partial_withdrawals = state.pending_partial_withdrawals().clone();
        let remaining: Vec<_> = partial_withdrawals
            .into_iter()
            .skip(partial_withdrawals_count)
            .cloned()
            .collect();
        *state.pending_partial_withdrawals_mut() = ssz::PersistentList::try_from_iter(remaining)?;
    }

    if let Some(latest_withdrawal) = expected_withdrawals.last() {
        *state.next_withdrawal_index_mut() = latest_withdrawal.index + 1;

        if expected_withdrawals.len() == P::MaxWithdrawalsPerPayload::USIZE {
            let next_validator_index = (latest_withdrawal.validator_index + 1) % state.validators().len_u64();
            *state.next_withdrawal_validator_index_mut() = next_validator_index;
        }
    }

    if expected_withdrawals.len() != P::MaxWithdrawalsPerPayload::USIZE {
        let next_validator_index = (state.next_withdrawal_validator_index() + P::MAX_VALIDATORS_PER_WITHDRAWALS_SWEEP) % state.validators().len_u64();
        *state.next_withdrawal_validator_index_mut() = next_validator_index;
    }

    Ok(())
}

pub fn process_withdrawals<P: Preset>(
    state: &mut impl PostEip7732BeaconState<P>,
) -> Result<()> {

    let (expected_withdrawals, builder_withdrawals_count, partial_withdrawals_count) = 
        get_expected_withdrawals(state)?;

    use ssz::{ContiguousList, SszHash as _};
    use tap::TryConv as _;
    let withdrawals_list = expected_withdrawals
        .clone()
        .try_conv::<ContiguousList<_, P::MaxWithdrawalsPerPayload>>()?;
    *state.latest_withdrawals_root_mut() = withdrawals_list.hash_tree_root();

    for withdrawal in expected_withdrawals.iter() {
        decrease_balance(
            balance(state, withdrawal.validator_index)?,
            withdrawal.amount,
        );
    }

    if builder_withdrawals_count > 0 {
        let builder_withdrawals = state.builder_pending_withdrawals().clone();
        let mut updated_builder_withdrawals = Vec::new();

        for (i, withdrawal) in builder_withdrawals.into_iter().enumerate() {
            if i < builder_withdrawals_count {
                if !is_builder_payment_withdrawable(state, &withdrawal)? {
                    updated_builder_withdrawals.push(withdrawal.clone());
                }
            } else {
                updated_builder_withdrawals.push(withdrawal.clone());
            }
        }

        *state.builder_pending_withdrawals_mut() = ssz::PersistentList::try_from_iter(updated_builder_withdrawals)?;
    }

    process_withdrawals_common(state, expected_withdrawals, Some(builder_withdrawals_count), Some(partial_withdrawals_count))?;

    Ok(())
}

pub fn verify_execution_payload_header_signature<P: Preset>(
    config: &Config,
    pubkey_cache: &PubkeyCache,
    state: &impl PostEip7732BeaconState<P>,
    signed_header: &SignedExecutionPayloadHeader,
    mut verifier: impl Verifier,
) -> Result<()> {
    let builder = state.validators()
        .get(signed_header.message.builder_index)
        .map_err(|_| anyhow::anyhow!("Invalid builder index"))?;
    
    verifier.verify_singular(
        signed_header.message.signing_root(config, state),
        signed_header.signature,
        pubkey_cache.get_or_insert(builder.pubkey)?,
        SignatureKind::BeaconBuilder,
    )?;
    
    Ok(())
}

pub fn process_execution_payload_header<P: Preset>(
    config: &Config,
    pubkey_cache: &PubkeyCache,
    state: &mut impl PostEip7732BeaconState<P>,
    block: &BeaconBlock<P>,
    verifier: impl Verifier,
) -> Result<()> {
    let signed_header = &block.body.signed_execution_payload_header;
    
    verify_execution_payload_header_signature(config, pubkey_cache, state, signed_header, verifier)?;
    
    let header = &signed_header.message;
    let builder_index = header.builder_index;
    let builder = state.validators()
        .get(builder_index)
        .map_err(|_| anyhow::anyhow!("Invalid builder index"))?;
    
    ensure!(
        is_active_validator(builder, get_current_epoch(state)),
        Error::<P>::BuilderNotActive { builder_index }
    );
    
    ensure!(!builder.slashed, Error::<P>::BuilderSlashed { builder_index });
    
    let amount = header.value;
    
    if builder_index == block.proposer_index {
        ensure!(amount == 0, Error::<P>::SelfBuildWithNonZeroValue);
    } else {
        ensure!(
            has_builder_withdrawal_credential(builder),
            Error::<P>::InvalidBuilderCredentials { builder_index }
        );
    }
    
    let pending_payments: Gwei = state.builder_pending_payments().into_iter()
        .filter(|p| p.withdrawal.builder_index == builder_index)
        .map(|p| p.withdrawal.amount)
        .sum();
    
    let pending_withdrawals: Gwei = state.builder_pending_withdrawals().into_iter()
        .filter(|w| w.builder_index == builder_index)
        .map(|w| w.amount)
        .sum();
    
    if amount > 0 {
        let builder_balance = state.balances()
            .get(builder_index)
            .map_err(|_| anyhow::anyhow!("Invalid builder index"))?;
        let required_balance = amount + pending_payments + pending_withdrawals + P::MIN_ACTIVATION_BALANCE;
        ensure!(
            *builder_balance >= required_balance,
            Error::<P>::InsufficientBuilderBalance { 
                builder_index, 
                balance: *builder_balance, 
                required: required_balance 
            }
        );
    }
    
    ensure!(header.slot == block.slot, Error::<P>::SlotMismatch { state_slot: block.slot, block_slot: header.slot });
    
    // Verify parent block hash
    ensure!(
        header.parent_block_hash == state.latest_block_hash(),
        Error::<P>::ParentBlockHashMismatch
    );
    ensure!(
        header.parent_block_root == block.parent_root,
        Error::<P>::ParentBlockRootMismatch
    );
    
    let _pending_payment = BuilderPendingPayment {
        weight: 0,
        withdrawal: BuilderPendingWithdrawal {
            fee_recipient: header.fee_recipient,
            amount,
            builder_index,
            withdrawable_epoch: (0),
        },
    };
    
    let _payment_index = 32 + (header.slot % 32) as usize;
    
    
    Ok(())
}

pub fn process_operations<P: Preset>(
    config: &Config,
    pubkey_cache: &PubkeyCache,
    state: &mut impl PostEip7732BeaconState<P>,
    body: &impl types::traits::PostEip7732BeaconBlockBody<P>,
    verifier: &mut impl Verifier,
    slot_report: &mut impl SlotReport,
) -> Result<()> {
    let eth1_deposit_index = state.eth1_deposit_index();
    let eth1_data = state.eth1_data();
    let expected_deposits = (eth1_data.deposit_count - eth1_deposit_index).min(P::MaxDeposits::U64);
    ensure!(
        body.deposits().len() == expected_deposits as usize,
        Error::<P>::DepositCountMismatch { computed: expected_deposits, in_block: body.deposits().len() as u64 }
    );
    
    for proposer_slashing in body.proposer_slashings().iter().copied() {
        bellatrix::process_proposer_slashing(
            config,
            pubkey_cache,
            state,
            proposer_slashing,
            &mut *verifier,
            &mut *slot_report,
        )?;
    }
    
    for attester_slashing in body.attester_slashings() {
        bellatrix::process_attester_slashing(
            config,
            pubkey_cache,
            state,
            attester_slashing,
            &mut *verifier,
            &mut *slot_report,
        )?;
    }
    
    for attestation in body.attestations() {
        process_attestation(config, pubkey_cache, state, attestation, &mut *verifier, &mut *slot_report)?;
    }
    
    if !body.deposits().is_empty() {
        let combined_deposits = unphased::validate_deposits(
            config,
            pubkey_cache,
            state,
            body.deposits().iter().copied(),
        )?;

        let deposit_count = body.deposits().len();
        
        *state.eth1_deposit_index_mut() += DepositIndex::try_from(deposit_count)?;
        
        altair::apply_deposits(state, deposit_count, combined_deposits, slot_report)?;
    }
    
    for voluntary_exit in body.voluntary_exits() {
        unphased::process_voluntary_exit(config, pubkey_cache, state, *voluntary_exit, &mut *verifier)?
    }
    
    for bls_to_execution_change in body.bls_to_execution_changes() {
        capella::process_bls_to_execution_change(config, pubkey_cache, state, *bls_to_execution_change, &mut *verifier)?;
    }
    
    for payload_attestation in body.payload_attestations() {
        process_payload_attestation(config, pubkey_cache, state, payload_attestation, &mut *verifier)?;
    }
    
    
    Ok(())
}

pub fn process_attestation<P: Preset>(
    config: &Config,
    pubkey_cache: &PubkeyCache,
    state: &mut impl PostEip7732BeaconState<P>,
    attestation: &types::electra::containers::Attestation<P>,
    verifier: &mut impl Verifier,
    slot_report: &mut impl SlotReport,
) -> Result<()> {
    let data = &attestation.data;
    
    let attestation_epoch = attestation_epoch(state, data.target.epoch)?;
    
    ensure!(
        data.target.epoch == compute_epoch_at_slot::<P>(data.slot),
        Error::<P>::AttestationTargetsWrongEpoch { attestation: attestation.clone().into() }
    );
    
    ensure!(
        data.slot + P::MIN_ATTESTATION_INCLUSION_DELAY.get() <= state.slot(),
        Error::<P>::AttestationOutsideInclusionRange {
            state_slot: state.slot(),
            attestation_slot: data.slot,
        }
    );
    
    ensure!(data.index < 2u64, Error::<P>::InvalidPayloadAvailabilityIndex);
    
    
    let inclusion_delay = state.slot() - data.slot;
    let participation_flags = get_attestation_participation_flags(state, data.clone(), inclusion_delay)?;
    
    let base_reward_per_increment = get_base_reward_per_increment(state);
    let attesting_indices_with_base_rewards = get_attesting_indices(state, attestation)?
        .into_iter()
        .map(|validator_index| {
            let base_reward = get_base_reward(state, validator_index, base_reward_per_increment)?;
            Ok((validator_index, base_reward))
        })
        .collect::<Result<Vec<_>>>()?;
    
    let payment_index = if matches!(attestation_epoch, AttestationEpoch::Current) {
        32 + (data.slot % 32) as usize
    } else {
        (data.slot % 32) as usize
    };
    
    let payments_binding = state.builder_pending_payments().clone();
    let payments_vec: Vec<_> = payments_binding.into_iter().collect();
    let payment = payments_vec.get(payment_index)
        .ok_or_else(|| anyhow::anyhow!("Invalid payment index"))?
        .clone();
    let mut weight_increase = 0u64;
    let mut proposer_reward_numerator = 0;
    
    for (validator_index, base_reward) in attesting_indices_with_base_rewards.iter() {
        let mut will_set_new_flag = false;
        let current_flags = match attestation_epoch {
            AttestationEpoch::Previous => state.previous_epoch_participation()
                .get(*validator_index)
                .map_err(|_| anyhow::anyhow!("Invalid validator index"))?,
            AttestationEpoch::Current => state.current_epoch_participation()
                .get(*validator_index)
                .map_err(|_| anyhow::anyhow!("Invalid validator index"))?,
        };
        
        for (flag_index, weight) in PARTICIPATION_FLAG_WEIGHTS {
            if participation_flags.get_bit(flag_index) && !current_flags.get_bit(flag_index) {
                proposer_reward_numerator += base_reward * weight;
                will_set_new_flag = true;
            }
        }
        
        if will_set_new_flag && is_attestation_same_slot(state, data)? {
            let validator = state.validators()
                .get(*validator_index)
                .map_err(|_| anyhow::anyhow!("Invalid validator index"))?;
            weight_increase += validator.effective_balance;
        }
    }
    
    let epoch_participation = match attestation_epoch {
        AttestationEpoch::Previous => state.previous_epoch_participation_mut(),
        AttestationEpoch::Current => state.current_epoch_participation_mut(),
    };
    
    for (validator_index, _) in attesting_indices_with_base_rewards {
        let epoch_participation = epoch_participation.get_mut(validator_index)?;
        *epoch_participation |= participation_flags;
    }
    
    let proposer_index = get_beacon_proposer_index(config, state)?;
    let proposer_reward_denominator = 
        (WEIGHT_DENOMINATOR.get() - PROPOSER_WEIGHT) * WEIGHT_DENOMINATOR.get() / PROPOSER_WEIGHT;
    let proposer_reward = proposer_reward_numerator / proposer_reward_denominator;
    
    increase_balance(balance(state, proposer_index)?, proposer_reward);
    
    let _ = payment;
    let _ = weight_increase;
    
    slot_report.add_attestation_reward(proposer_reward);
    slot_report.update_performance(
        state,
        attestation.data,
        get_attesting_indices(state, attestation)?,
    )?;
    
    Ok(())
}

pub fn process_payload_attestation<P: Preset>(
    config: &Config,
    pubkey_cache: &PubkeyCache,
    state: &mut impl PostEip7732BeaconState<P>,
    payload_attestation: &PayloadAttestation<P>,
    _verifier: impl Verifier,
) -> Result<()> {
    let data = &payload_attestation.data;
    
    ensure!(
        data.beacon_block_root == state.latest_block_header().parent_root,
        Error::<P>::PayloadAttestationNotForParentBlock
    );
    
    ensure!(
        data.slot + 1 == state.slot(),
        Error::<P>::PayloadAttestationNotForPreviousSlot
    );
    
    let indexed = get_indexed_payload_attestation(state, payload_attestation)?;
    
    ensure!(
        is_valid_indexed_payload_attestation(config, pubkey_cache, state, &indexed)?,
        Error::<P>::InvalidPayloadAttestation
    );
    
    Ok(())
}

pub fn verify_execution_payload_envelope_signature<P: Preset>(
    _config: &Config,
    _state: &impl PostEip7732BeaconState<P>,
    _signed_envelope: &SignedExecutionPayloadEnvelope<P>,
) -> Result<()> {
    Ok(())
}

pub fn process_execution_payload<P: Preset>(
    config: &Config,
    state: &mut impl PostEip7732BeaconState<P>,
    signed_envelope: &SignedExecutionPayloadEnvelope<P>,
    execution_engine: impl ExecutionEngine<P>,
    verify: bool,
) -> Result<()> {
    if verify {
        verify_execution_payload_envelope_signature(config, state, signed_envelope)?;
    }
    
    let envelope = &signed_envelope.message;
    let payload = &envelope.payload;
    
    let previous_state_root = state.hash_tree_root();
    if state.latest_block_header().state_root == H256::zero() {
        state.latest_block_header_mut().state_root = previous_state_root;
    }
    
    ensure!(
        envelope.beacon_block_root == state.latest_block_header().hash_tree_root(),
        Error::<P>::BeaconBlockRootMismatch
    );
    ensure!(envelope.slot == state.slot(), Error::<P>::SlotMismatch { state_slot: state.slot(), block_slot: envelope.slot });
    
    
    ensure!(
        payload.withdrawals.hash_tree_root() == state.latest_withdrawals_root(),
        Error::<P>::WithdrawalsRootMismatch
    );
    
    ensure!(payload.parent_hash == state.latest_block_hash(), Error::<P>::ParentHashMismatch);
    ensure!(
        payload.prev_randao == get_randao_mix(state, get_current_epoch(state)),
        Error::<P>::PrevRandaoMismatch
    );
    ensure!(
        payload.timestamp == compute_timestamp_at_slot(config, state, state.slot()),
        Error::<P>::TimestampMismatch
    );
    
    let max_blobs = config.max_blobs_per_block_electra;
    ensure!(
        envelope.blob_kzg_commitments.len() <= max_blobs,
        Error::<P>::TooManyBlobCommitments
    );
    
    let versioned_hashes: Vec<_> = envelope.blob_kzg_commitments.iter()
        .map(|commitment| kzg_commitment_to_versioned_hash(*commitment))
        .collect();
    
    
    
    let payment_index = 32 + (state.slot() % 32) as usize;
    let payments_binding = state.builder_pending_payments().clone();
    let payments_vec: Vec<_> = payments_binding.into_iter().collect();
    let payment = payments_vec.get(payment_index)
        .ok_or_else(|| anyhow::anyhow!("Invalid payment index"))?;
    
    let exit_queue_epoch = compute_exit_epoch_and_update_churn(config, state, payment.withdrawal.amount);
    let mut updated_payment = (*payment).clone();
    updated_payment.withdrawal.withdrawable_epoch = 
        exit_queue_epoch + config.min_validator_withdrawability_delay;
    
    let builder_withdrawals_list = state.builder_pending_withdrawals().clone();
    let mut builder_withdrawals: Vec<BuilderPendingWithdrawal> = builder_withdrawals_list.into_iter().cloned().collect();
    builder_withdrawals.push(updated_payment.withdrawal);
    *state.builder_pending_withdrawals_mut() = ssz::PersistentList::try_from_iter(builder_withdrawals.into_iter())?;
    
    
    use bit_field::BitField;
    let slot_index = (state.slot() % 8192) as usize;
    state.execution_payload_availability_mut().set(slot_index, true);
    *state.latest_block_hash_mut() = payload.block_hash;
    
    
    Ok(())
}