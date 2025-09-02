use anyhow::Result;
use core::num::NonZeroU64;
use ssz::{BitVector, ContiguousVector, ContiguousList};
use tap::{Pipe as _, TryConv as _};
use try_from_iterator::TryFromIterator as _;
use typenum::U512;
use types::{
    eip7732::{
        containers::{Ptc, IndexedPayloadAttestation, PayloadAttestation},
        consts::DOMAIN_PTC_ATTESTER,
    },
    phase0::primitives::{Slot, ValidatorIndex, H256},
    preset::Preset,
    traits::BeaconState,
};

use crate::{
    accessors::{get_seed_by_epoch, beacon_committees},
    misc::{compute_epoch_at_slot, compute_shuffled_index},
    error::Error,
};

pub fn get_ptc_attester_seed<P: Preset>(
    state: &impl BeaconState<P>, 
    slot: Slot,
) -> Result<H256> {
    let epoch = compute_epoch_at_slot::<P>(slot);
    
    let base_seed = get_seed_by_epoch(state, epoch, DOMAIN_PTC_ATTESTER);
    
    // Hash base_seed with slot to get unique seed per slot
    // Using Grandine's hashing pattern: hash_256_64 takes H256 and u64
    let seed = hashing::hash_256_64(base_seed, slot);
    
    Ok(seed)
}
pub fn get_ptc<P: Preset>(
    state: &impl BeaconState<P>,
    slot: Slot,
) -> Result<Ptc<P>> {
    // Get all committees at the slot
    let committees_iter = beacon_committees(state, slot)?;
    
    // Concatenate all committee members
    let mut all_validators_at_slot = Vec::new();
    for committee in committees_iter {
        // IndexSlice implements IntoIterator, no need to call .iter()
        all_validators_at_slot.extend(committee);
    }
    
    // Get PTC seed for this slot
    let seed = get_ptc_attester_seed::<P>(state, slot)?;
    
    // Use balance-weighted selection to sample PTC_SIZE validators
    let ptc_size = 512; // PTC_SIZE constant
    let selected = compute_balance_weighted_selection::<P>(
        state,
        &all_validators_at_slot,
        seed,
        ptc_size,
        false,  // Don't shuffle indices for PTC
    )?;
    
    // Convert to ContiguousVector
    let indices = ContiguousVector::<ValidatorIndex, U512>::try_from_iter(selected)
        .map_err(|_| anyhow::anyhow!("Failed to create PTC indices"))?;
    
    // Create Ptc struct using Default and then setting the field
    let mut ptc = Ptc::<P>::default();
    ptc.indices = indices;
    Ok(ptc)
}

pub fn compute_balance_weighted_selection<P: Preset>(
    state: &(impl BeaconState<P> + ?Sized),
    indices: &[ValidatorIndex],
    seed: H256,
    size: usize,
    shuffle_indices: bool,
) -> Result<Vec<ValidatorIndex>> {
    let total = indices
        .len()
        .try_conv::<u64>()?
        .pipe(NonZeroU64::new)
        .ok_or(Error::NoActiveValidators)?;
    
    let mut selected = Vec::with_capacity(size);
    let mut count = 0u64;
    
    while selected.len() < size {
        let mut next_index = count % total.get();
        
        if shuffle_indices {
            next_index = compute_shuffled_index::<P>(
                next_index,
                total,
                seed,
            );
        }
        
        let candidate_index = indices[next_index as usize];
        
        if compute_balance_weighted_acceptance::<P>(
            state,
            candidate_index,
            seed,
            count,
        )? {
            selected.push(candidate_index);
        }
        
        count += 1;
        
        if count > total.get() * 1000 {
            return Err(anyhow::anyhow!("Selection timeout: unable to select enough validators"));
        }
    }
    
    Ok(selected)
}

pub fn compute_balance_weighted_acceptance<P: Preset>(
    state: &(impl BeaconState<P> + ?Sized),
    validator_index: ValidatorIndex,
    seed: H256,
    iteration: u64,
) -> Result<bool> {
    let validator = state.validators()
        .get(validator_index)
        .map_err(|_| anyhow::anyhow!("Invalid validator index"))?;
    let effective_balance = validator.effective_balance;
    
    let max_effective_balance = if state.is_post_electra() {
        P::MAX_EFFECTIVE_BALANCE_ELECTRA
    } else {
        P::MAX_EFFECTIVE_BALANCE
    };
    
    let random_value = if state.is_post_electra() {
        let random_bytes = hashing::hash_256_64(seed, iteration / 16);
        let offset = ((iteration % 16) * 2) as usize;
        let bytes = random_bytes.as_bytes();
        u64::from(u16::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
        ]))
    } else {
        let random_bytes = hashing::hash_256_64(seed, iteration / 32);
        let offset = (iteration % 32) as usize;
        u64::from(random_bytes.as_bytes()[offset])
    };
    
    let max_random_value = if state.is_post_electra() {
        (1u64 << 16) - 1
    } else {
        255u64
    };
    
    let accepted = effective_balance * max_random_value >= max_effective_balance * random_value;
    
    Ok(accepted)
}

pub fn get_payload_attesting_indices<P: Preset>(
    state: &impl BeaconState<P>,
    slot: Slot,
    aggregation_bits: &BitVector<P::PtcSize>,
) -> Result<Vec<ValidatorIndex>> {
    let ptc = get_ptc(state, slot)?;
    
    let attesting_indices: Vec<ValidatorIndex> = aggregation_bits
        .into_iter()
        .zip(ptc.indices.iter())
        .filter_map(|(bit, &validator_index)| bit.then_some(validator_index))
        .collect();
    
    Ok(attesting_indices)
}

  pub fn get_indexed_payload_attestation<P: Preset>(
      state: &impl BeaconState<P>,
      payload_attestation: &PayloadAttestation<P>,
  ) -> Result<IndexedPayloadAttestation> {
      let attesting_indices_iter =
          get_payload_attesting_indices(state, payload_attestation.data.slot, &payload_attestation.aggregation_bits)?;

      let mut attesting_indices = ContiguousList::try_from_iter(attesting_indices_iter).expect(
          "PayloadAttestation.aggregation_bits and IndexedPayloadAttestation.attesting_indices \
           have compatible lengths (PTC_SIZE to MAX_VALIDATORS_PER_COMMITTEE)",
      );

      // Sorting a slice is faster than building a `BTreeMap`.
      attesting_indices.sort_unstable();

      Ok(IndexedPayloadAttestation {
          attesting_indices,
          data: payload_attestation.data.clone(),
          signature: payload_attestation.signature,
      })
  }