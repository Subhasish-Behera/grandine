# ePBS Implementation Tasks for Grandine v1

## Overview
This document outlines the comprehensive implementation plan for enshrined Proposer-Builder Separation (ePBS) in Grandine, based on analysis of the current codebase and Ethereum specifications.

## Major Sources and References

### Primary Analysis Sources
- **Main Document**: `/home/greendior/grandine/docs/EPBS_IMPLEMENTATION_ANALYSIS.md` (comprehensive 635-line analysis)
- **Architecture Analysis**: `/home/greendior/grandine/docs/ARCHITECTURE_MASTER.md`
- **Layer Analysis**: `/home/greendior/grandine/docs/layer_architecture.md`
- **Fork Choice Analysis**: `/home/greendior/grandine/docs/fork_choice_analysis.md`
- **Networking Analysis**: `/home/greendior/grandine/docs/networking_analysis.md`

### Ethereum Specifications
- Ethereum ePBS specification (referenced in implementation analysis)
- Prysm ePBS implementation (used as reference architecture)

### Current Grandine Codebase Analysis
- 60+ crate modular architecture
- Generic type system with Preset trait
- Existing fork choice implementation in `fork_choice_store/`
- P2P networking in `eth2_libp2p/` and `p2p/`

---

## Implementation Tasks by Phase

### Phase 1: Foundation (4-6 weeks)

#### Task 1.1: Core Type System Extensions
**Location**: `types/src/`
**Estimated Effort**: 20-30 hours

**Files to Create**:
```
types/src/epbs/
├── execution_payload_header.rs
├── payload_attestation.rs  
├── blind_payload_envelope.rs
├── ptc_status.rs
└── epbs_primitives.rs
```

**Key Implementation**:
```rust
// types/src/epbs/execution_payload_header.rs
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ssz)]
pub struct ExecutionPayloadHeader {
    pub parent_hash: Hash256,
    pub fee_recipient: Address,
    pub state_root: Hash256,
    pub receipts_root: Hash256,
    pub logs_bloom: FixedVector<u8, 256>,
    pub prev_randao: Hash256,
    pub block_number: u64,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub timestamp: u64,
    pub extra_data: VariableList<u8, 32>,
    pub base_fee_per_gas: U256,
    pub block_hash: Hash256,
    pub transactions_root: Hash256,
    pub withdrawals_root: Hash256,
    pub blob_gas_used: u64,
    pub excess_blob_gas: u64,
}

// types/src/epbs/payload_attestation.rs
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ssz)]
pub struct PayloadAttestation {
    pub slot: Slot,
    pub validator_index: ValidatorIndex,
    pub payload_root: Hash256,
    pub signature: BlsSignature,
}

// types/src/epbs/blind_payload_envelope.rs
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ssz)]
pub struct BlindPayloadEnvelope {
    pub payload_header: ExecutionPayloadHeader,
    pub builder_index: ValidatorIndex,
    pub builder_signature: BlsSignature,
}

// types/src/epbs/ptc_status.rs
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ssz)]
pub struct PTCStatus {
    pub payload_withhold_index: u8,
    pub payload_inclusion_delay: u8,
}
```

**Source**: Lines 84-125 in `docs/EPBS_IMPLEMENTATION_ANALYSIS.md`

#### Task 1.2: Extend Electra Container Types
**Location**: `types/src/electra/containers.rs`
**Estimated Effort**: 15-20 hours

**Modifications Required**:
```rust
// Extend BeaconBlockBodyEpbs
pub struct BeaconBlockBodyEpbs<P: Preset> {
    // Existing Electra fields...
    pub execution_payload_header: ExecutionPayloadHeader,
    pub payload_attestations: List<PayloadAttestation, P::MaxPayloadAttestations>,
    pub blind_payload_envelope: BlindPayloadEnvelope,
}

// Extend BeaconStateEpbs  
pub struct BeaconStateEpbs<P: Preset> {
    // Existing state fields...
    pub latest_execution_payload_header: ExecutionPayloadHeader,
    pub payload_attestations: List<PayloadAttestation, P::MaxPayloadAttestations>,
    pub ptc_status: PTCStatus,
    pub next_payload_id: u64,
}
```

**Source**: Lines 95-125 in `docs/EPBS_IMPLEMENTATION_ANALYSIS.md`

#### Task 1.3: Configuration Updates
**Location**: Various preset and config files
**Estimated Effort**: 10-15 hours

**Files to Modify**:
- `types/src/preset.rs` - Add ePBS-specific constants
- `types/src/config.rs` - Add fork version handling
- `grandine/src/predefined_network.rs` - Add ePBS network configs

### Phase 2: Core Logic (6-8 weeks)

#### Task 2.1: State Transition Functions
**Location**: `transition_functions/src/electra/`
**Estimated Effort**: 40-60 hours

**New Modules to Create**:
```
transition_functions/src/electra/epbs/
├── process_execution_payload_header.rs
├── process_payload_attestations.rs
├── process_blind_payload_envelope.rs
└── epbs_epoch_processing.rs
```

**Key Implementation Example**:
```rust
// transition_functions/src/electra/epbs/process_payload_attestations.rs
pub fn process_payload_attestations<P: Preset>(
    state: &mut BeaconStateEpbs<P>,
    payload_attestations: &[PayloadAttestation],
) -> Result<()> {
    for attestation in payload_attestations {
        verify_payload_attestation_signature(state, attestation)?;
        update_ptc_status(state, attestation)?;
        state.payload_attestations.push(*attestation)?;
    }
    Ok(())
}
```

**Source**: Lines 130-160 in `docs/EPBS_IMPLEMENTATION_ANALYSIS.md`

#### Task 2.2: Fork Choice Store Modifications  
**Location**: `fork_choice_store/src/`
**Estimated Effort**: 50-70 hours

**New File**: `fork_choice_store/src/epbs_store.rs`
```rust
pub struct EpbsStore<P: Preset> {
    payload_attestations: HashMap<H256, Vec<PayloadAttestation>>,
    ptc_votes: HashMap<ValidatorIndex, PTCVote>,
    blind_payload_envelopes: HashMap<H256, BlindPayloadEnvelope>,
    execution_payload_headers: HashMap<H256, ExecutionPayloadHeader>,
}
```

**Modifications to**: `fork_choice_store/src/store.rs`
```rust
impl<P: Preset, S: StoreConfig> Store<P, S> {
    pub fn process_payload_attestation(
        &mut self,
        payload_attestation: &PayloadAttestation,
    ) -> Result<()> {
        self.validate_payload_attestation(payload_attestation)?;
        self.update_payload_votes(payload_attestation)?;
        self.update_head_if_needed()?;
        Ok(())
    }
    
    pub fn get_head_with_epbs(&self) -> Result<H256> {
        self.get_head_with_payload_attestations()
    }
}
```

**Source**: Lines 162-209 in `docs/EPBS_IMPLEMENTATION_ANALYSIS.md`

### Phase 3: Integration (4-6 weeks)

#### Task 3.1: Fork Choice Control Integration
**Location**: `fork_choice_control/src/controller.rs`
**Estimated Effort**: 30-40 hours

**New Message Types**:
```rust
pub enum EpbsMessage<P: Preset> {
    PayloadAttestation(PayloadAttestation),
    BlindPayloadEnvelope(BlindPayloadEnvelope),
    ExecutionPayloadHeader(ExecutionPayloadHeader),
}

impl<P, E, A, W> Controller<P, E, A, W> {
    pub fn on_payload_attestation(
        &self,
        payload_attestation: PayloadAttestation,
    ) -> Result<()> {
        self.validate_payload_attestation(&payload_attestation)?;
        self.mutator_tx.send(MutatorMessage::PayloadAttestation(payload_attestation))?;
        Ok(())
    }
}
```

**Source**: Lines 211-239 in `docs/EPBS_IMPLEMENTATION_ANALYSIS.md`

#### Task 3.2: Networking Layer Changes
**Location**: `p2p/src/` and `eth2_libp2p/src/`
**Estimated Effort**: 35-45 hours

**New File**: `p2p/src/epbs_messages.rs`
```rust
pub enum EpbsP2pMessage<P: Preset> {
    PayloadAttestation(PayloadAttestation),
    BlindPayloadEnvelope(BlindPayloadEnvelope),
    ExecutionPayloadHeader(ExecutionPayloadHeader),
}
```

**Modifications to**: `p2p/src/network.rs`
```rust
impl<P: Preset> Network<P> {
    fn handle_epbs_message(
        &mut self,
        message: EpbsP2pMessage<P>,
    ) -> Result<()> {
        match message {
            EpbsP2pMessage::PayloadAttestation(attestation) => {
                self.channels.fork_choice_tx.send(
                    ForkChoiceMessage::PayloadAttestation(attestation)
                )?;
            }
            // Handle other ePBS message types...
        }
        Ok(())
    }
}
```

**Source**: Lines 241-285 in `docs/EPBS_IMPLEMENTATION_ANALYSIS.md`

#### Task 3.3: Database Schema Changes
**Location**: `database/src/`
**Estimated Effort**: 25-35 hours

**New File**: `database/src/epbs_schema.rs`
```rust
pub const PAYLOAD_ATTESTATIONS_PREFIX: &[u8] = b"payload_attestations";
pub const BLIND_PAYLOAD_ENVELOPES_PREFIX: &[u8] = b"blind_payload_envelopes";
pub const EXECUTION_PAYLOAD_HEADERS_PREFIX: &[u8] = b"execution_payload_headers";
pub const PTC_STATUS_PREFIX: &[u8] = b"ptc_status";

impl Database {
    pub fn store_payload_attestation(
        &self,
        slot: Slot,
        attestation: &PayloadAttestation,
    ) -> Result<()> {
        let key = format!("{:016x}_{:016x}", slot, attestation.validator_index);
        self.put(
            &[PAYLOAD_ATTESTATIONS_PREFIX, key.as_bytes()].concat(),
            &attestation.as_ssz_bytes(),
        )
    }
    
    pub fn get_payload_attestations(
        &self,
        slot: Slot,
    ) -> Result<Vec<PayloadAttestation>> {
        // Implementation for retrieving payload attestations
    }
}
```

**Source**: Lines 349-390 in `docs/EPBS_IMPLEMENTATION_ANALYSIS.md`

### Phase 4: Application Layer (3-4 weeks)

#### Task 4.1: Validator Client Changes
**Location**: `validator/src/`
**Estimated Effort**: 30-40 hours

**New File**: `validator/src/epbs_duties.rs`
```rust
pub struct EpbsDuties<P: Preset> {
    payload_attestation_duties: HashMap<Slot, Vec<PayloadAttestationDuty>>,
    ptc_committee_duties: HashMap<Epoch, Vec<PTCCommitteeDuty>>,
}

impl<P: Preset> EpbsDuties<P> {
    pub fn create_payload_attestation(
        &self,
        duty: &PayloadAttestationDuty,
        payload_header: &ExecutionPayloadHeader,
    ) -> Result<PayloadAttestation> {
        let attestation = PayloadAttestation {
            slot: duty.slot,
            validator_index: duty.validator_index,
            payload_root: payload_header.hash_tree_root(),
            signature: BlsSignature::empty(), // Will be signed later
        };
        Ok(attestation)
    }
}
```

**Source**: Lines 287-326 in `docs/EPBS_IMPLEMENTATION_ANALYSIS.md`

#### Task 4.2: HTTP API Extensions
**Location**: `http_api/src/`
**Estimated Effort**: 20-30 hours

**New File**: `http_api/src/epbs_endpoints.rs`
```rust
pub fn register_epbs_routes<P: Preset>(
    router: axum::Router,
    controller: ApiController<P>,
) -> axum::Router {
    router
        .route("/eth/v1/beacon/payload_attestations", get(get_payload_attestations))
        .route("/eth/v1/beacon/blind_payload_envelopes", get(get_blind_payload_envelopes))
        .route("/eth/v1/beacon/execution_payload_headers", get(get_execution_payload_headers))
        .route("/eth/v1/validator/payload_attestation_duties/:epoch", get(get_payload_attestation_duties))
        .with_state(controller)
}
```

**Source**: Lines 328-346 in `docs/EPBS_IMPLEMENTATION_ANALYSIS.md`

### Phase 5: Testing & Optimization (4-6 weeks)

#### Task 5.1: Comprehensive Testing
**Estimated Effort**: 60-80 hours

**Test Categories**:
1. **Unit Tests** - All new ePBS types and functions
2. **Integration Tests** - End-to-end ePBS block processing
3. **Specification Compliance** - Official ePBS spec tests
4. **Performance Tests** - Memory usage and CPU benchmarks
5. **Compatibility Tests** - Pre-ePBS block handling

**Source**: Lines 519-550 in `docs/EPBS_IMPLEMENTATION_ANALYSIS.md`

#### Task 5.2: Performance Optimization
**Estimated Effort**: 40-60 hours

**Optimization Areas**:
- Memory usage (target +500MB increase, 20% over current)
- CPU performance (+12-22% usage target)
- Database operations (+1.8GB/month storage)
- Network message processing

**Source**: Lines 481-516 in `docs/EPBS_IMPLEMENTATION_ANALYSIS.md`

---

## Impact Analysis

### Files Requiring Major Changes (20+ hours each)
1. `types/src/electra/beacon_state.rs` - New state fields
2. `types/src/electra/containers.rs` - New container types  
3. `fork_choice_store/src/store.rs` - Fork choice extensions
4. `transition_functions/src/electra/block_processing.rs` - Block processing
5. `validator/src/validator.rs` - Validator duties

### Files Requiring Medium Changes (10-20 hours each)
1. `p2p/src/network.rs` - Network message handling
2. `database/src/lib.rs` - Database schema updates
3. `http_api/src/lib.rs` - API endpoint additions
4. `runtime/src/runtime.rs` - Service orchestration
5. `grandine/src/main.rs` - Configuration updates

### Files Requiring Minor Changes (5-10 hours each)
1. `ssz/src/lib.rs` - SSZ serialization support
2. `hashing/src/lib.rs` - Hash tree root updates
3. `metrics/src/lib.rs` - Metrics additions
4. Various test files - Test updates

**Source**: Lines 613-635 in `docs/EPBS_IMPLEMENTATION_ANALYSIS.md`

---

## Development Estimates

### Timeline and Resources
- **Total Estimated Timeline**: 20-28 weeks
- **Team Size Required**: 4-6 experienced Rust developers  
- **Code Impact**: ~10,000 new lines, ~5,000 modified lines
- **Files Modified**: 60+ files across all major components
- **New Modules**: 15+ new modules for ePBS-specific functionality

### Resource Impact
- **Memory Usage**: +500MB (+20% increase)
- **CPU Usage**: +12-22% increase
- **Storage**: +1.8GB/month additional
- **Network**: Additional message types and processing

**Source**: Lines 582-608 in `docs/EPBS_IMPLEMENTATION_ANALYSIS.md`

---

## Key Technical Challenges

### 1. State Root Calculation Complexity
- ePBS adds new state fields affecting state root calculation
- Requires careful SSZ implementation
- Performance implications for large validator sets

### 2. Fork Choice Weight Calculation  
- Payload attestations add complexity to vote counting
- Mixed-mode blocks require special handling
- Performance critical for head selection

### 3. Database Migration
- Existing databases need schema updates
- Migration path for live networks
- Backward compatibility requirements

### 4. Memory Management
- Additional state fields increase memory usage
- Payload attestation tracking requires efficient storage
- Cache invalidation becomes more complex

### 5. Network Protocol Changes
- New message types require protocol updates
- Backward compatibility during transition
- Gossip protocol modifications

**Source**: Lines 453-479 in `docs/EPBS_IMPLEMENTATION_ANALYSIS.md`

---

## Migration Strategy

### Development Environment
1. Create ePBS feature branch
2. Add feature flags for ePBS
3. Implement behind configuration gates

### Testnet Deployment
1. Deploy on dedicated ePBS testnet
2. Validate basic functionality  
3. Performance testing and optimization

### Public Testnet
1. Deploy on public testnets (Holesky, Sepolia)
2. Community testing and feedback
3. Bug fixes and improvements

### Mainnet Preparation
1. Security audits
2. Performance optimization
3. Final testing and validation

### Mainnet Activation
1. Coordinated fork activation
2. Monitoring and support
3. Post-activation optimization

**Source**: Lines 553-579 in `docs/EPBS_IMPLEMENTATION_ANALYSIS.md`

---

## Conclusion

This comprehensive ePBS implementation represents one of the most significant protocol changes since the Merge. The modular architecture of Grandine provides a solid foundation, but the scope requires substantial development investment across virtually every layer of the consensus client.

The implementation would position Grandine as a cutting-edge consensus client with full ePBS support, enabling improved MEV efficiency, enhanced proposer-builder separation, and future-proofing for additional PBS improvements.

**Primary Source**: All implementation details derived from comprehensive analysis in `/home/greendior/grandine/docs/EPBS_IMPLEMENTATION_ANALYSIS.md` (635 lines) and supporting architecture documentation.