use std::sync::Arc;

use anyhow::{bail, ensure, Error as AnyhowError, Result};
use eth1_api::{ApiController, RealController};
use fork_choice_control::Wait;
use helper_functions::{accessors, misc};
use logging::debug_with_peers;
use ssz::BitVector;
use typenum::Unsigned as _;
use types::{
    combined::{Attestation, BeaconState},
    electra::{
        containers::{Attestation as ElectraAttestation, SingleAttestation},
        error::AttestationConversionError,
    },
    phase0::containers::{Attestation as Phase0Attestation, AttestationData},
    phase0::primitives::CommitteeIndex,
    preset::Preset,
};

pub fn convert_attestation_for_pool<P: Preset, W: Wait>(
    controller: &ApiController<P, W>,
    attestation: Arc<Attestation<P>>,
) -> Result<Phase0Attestation<P>> {
    if attestation.data().slot + P::SlotsPerEpoch::U64 < controller.slot() {
        bail!(AttestationConversionError::Irrelevant);
    }

    let attestation = match Arc::unwrap_or_clone(attestation) {
        Attestation::Phase0(attestation) => attestation,
        Attestation::Electra(attestation) => {
            let ElectraAttestation {
                aggregation_bits,
                data,
                committee_bits,
                signature,
            } = attestation;

            let aggregation_bits: Vec<u8> = aggregation_bits.into();

            let index = misc::get_committee_indices::<P>(committee_bits)
                .next()
                .ok_or(AttestationConversionError::InvalidCommitteeIndex)?;

            Phase0Attestation {
                aggregation_bits: aggregation_bits
                    .try_into()
                    .map_err(AttestationConversionError::InvalidAggregationBits)?,
                data: AttestationData { index, ..data },
                signature,
            }
        }
        Attestation::Single(attestation) => {
            let slot = attestation.data.slot;
            let state = current_state(controller);
            let committee = accessors::beacon_committee(&state, slot, attestation.committee_index)?;

            attestation.try_into_phase0_attestation(committee)?
        }
    };

    Ok(attestation)
}

pub fn convert_to_electra_attestation<P: Preset>(
    attestation: Phase0Attestation<P>,
) -> Result<ElectraAttestation<P>> {
    attestation.try_into()
}
//dev notes: can not use the try_into implmentation used in convert_to_electra_attestations anymore
//because it had no logic/acccess to the state on how to set data.index(which can be both 1 and 0). it alwyas sets to 0.
// but here when u are getting attestation from the pool, the data.index, the rest 2 fields are supplied from the caller
// restored index is the actualy indirect execution_payload_availability in gloas path. at both call sites
// why commitee index is needed as a field: it is not needed when the caller is block producer(because its in pool format there)
// but its required in validatrors aggregate path because therer attestation.data.index has become payload status. so to be used in both
// sites, its taking comitee index as a fields as well.

pub fn convert_to_electra_attestation_with_committee_index<P: Preset>(
    attestation: Phase0Attestation<P>,
    committee_index: CommitteeIndex,
    restored_index: CommitteeIndex,
) -> Result<ElectraAttestation<P>> {
    let Phase0Attestation {
        aggregation_bits,
        data,
        signature,
    } = attestation;

    ensure!(
        committee_index < P::MaxCommitteesPerSlot::U64,
        AttestationConversionError::InvalidCommitteeIndex
    );

    let aggregation_bits: Vec<u8> = aggregation_bits.into();
    let mut committee_bits = BitVector::default();
    committee_bits.set(committee_index.try_into()?, true);

    // Restore the correct data.index for the Electra attestation.
    // In the pool, data.index = committee_index. The caller provides the correct
    // restored_index: 0 for pre-Gloas Electra, payload_status for Gloas.
    let data = AttestationData { index: restored_index, ..data };

    Ok(ElectraAttestation {
        aggregation_bits: aggregation_bits
            .try_into()
            .map_err(AttestationConversionError::InvalidAggregationBits)?,
        data,
        committee_bits,
        signature,
    })
}

// TODO(feature/electra): properly refactor attestations
pub fn try_convert_to_single_attestation<P: Preset>(
    controller: &RealController<P>,
    attestation: ElectraAttestation<P>,
) -> Result<SingleAttestation> {
    let ElectraAttestation {
        aggregation_bits,
        data,
        signature,
        committee_bits,
    } = attestation;

    let committee_index = misc::get_committee_indices::<P>(committee_bits)
        .next()
        .unwrap_or_default();

    let state = current_state(controller);
    let committee = accessors::beacon_committee(&state, data.slot, committee_index)?;

    let attester_index = aggregation_bits
        .iter()
        .zip(committee)
        .find_map(|(participated, validator_index)| (*participated).then_some(validator_index))
        .ok_or_else(|| AnyhowError::msg("attester_index not available"))?;

    Ok(SingleAttestation {
        committee_index,
        attester_index,
        data,
        signature,
    })
}

fn current_state<P: Preset, W: Wait>(controller: &ApiController<P, W>) -> Arc<BeaconState<P>> {
    if !controller.is_forward_synced() {
        return controller.head_state().value;
    }

    match controller.preprocessed_state_at_current_slot_blocking() {
        Ok(state) => state,
        Err(error) => {
            debug_with_peers!(
                "failed to get state at current slot for attestation conversion: {error}. \
                 Using head state instead",
            );

            controller.head_state().value
        }
    }
}
