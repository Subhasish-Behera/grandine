# ePBS Fork Choice Implementation for Grandine

## Overview
Implementing ePBS (enshrined Proposer-Builder Separation) dual-variant fork choice architecture inspired by Prysm's doubly-linked-tree approach, adapted to Grandine's segment-based architecture.

## Core Problem
In ePBS, the same block can exist in two states:
1. **Empty variant**: Beacon block received, but execution payload not yet available
2. **Full variant**: Beacon block + execution payload both available

Both variants must coexist in fork choice, with separate weights, and fork choice must pick the variant with higher weight.

## Key Architectural Decisions

### 1. BlockNode Structure (Shared Metadata)
**From Prysm**: `BlockNode` is shared between empty and full `Node` objects via pointer.

**Grandine Implementation**:
```rust
pub struct BlockNode<P: Preset> {
    pub block_root: H256,
    pub block: Arc<SignedBeaconBlock<P>>,
    pub total_weight: RwLock<Gwei>,  // Aggregates both empty + full weights
}
```

**Rationale**:
- Block metadata (root, block object) is truly **block-specific**, not chain-specific
- `total_weight` aggregates weights from both variants for comparison
- `RwLock` allows thread-safe updates from weight propagation
- Checkpoints are **NOT** here because they are chain-specific (depend on which parent the block built upon)

### 2. ChainLink Structure (Variant + Chain Specific)
```rust
pub struct ChainLink<P: Preset> {
    pub block_node: Arc<BlockNode<P>>,  // Shared reference

    // Variant-specific states
    pub block_state: Option<Arc<BeaconState<P>>>,      // Empty variant
    pub execution_state: Option<Arc<BeaconState<P>>>,  // Full variant
    pub is_full: bool,

    // Chain-specific checkpoints
    pub current_justified_checkpoint: Checkpoint,
    pub finalized_checkpoint: Checkpoint,
    pub unrealized_justified_checkpoint: Checkpoint,
    pub unrealized_finalized_checkpoint: Checkpoint,

    // EL validation status
    pub payload_status: PayloadStatus,  // Valid/Invalid/Optimistic
}
```

**Rationale for checkpoint placement**:
- Checkpoints depend on **ancestry** (which parent the block built on)
- Same block in different forks can have different checkpoints
- Therefore checkpoints are **chain-specific**, not block-specific
- LMD GHOST doesn't use checkpoints directly (Casper FFG does)

### 3. Store Structure (Dual Tracking)
**From Prysm**: Separate maps `emptyNodeByRoot` and `fullNodeByPayload`

**Grandine Implementation**:
```rust
pub struct Store<P: Preset, S: Storage<P>> {
    block_nodes: HashMap<H256, Arc<BlockNode<P>>>,  // Shared metadata by block_root

    // Segments contain BOTH empty and full variants as separate UnfinalizedBlock entries
    unfinalized: OrdMap<SegmentId, Segment<P>>,

    // Dual location tracking
    unfinalized_locations: HashMap<H256, Location>,       // Empty variant
    unfinalized_full_locations: HashMap<H256, Location>,  // Full variant
}
```

**Key difference from Prysm**:
- Prysm: Dual separate trees (`emptyNodeByRoot`, `fullNodeByPayload`)
- Grandine: Single segment structure, but segments can contain **both variants** of same block

### 4. Segment Structure (Both Variants Coexist)
```rust
Segment {
    blocks: Vector<UnfinalizedBlock> [
        UnfinalizedBlock { chain_link: ChainLink(block_node: Arc(BlockNode_100), is_full: false), balance: 1000 },
        UnfinalizedBlock { chain_link: ChainLink(block_node: Arc(BlockNode_100), is_full: true),  balance: 500 },  // SAME BlockNode!
        UnfinalizedBlock { chain_link: ChainLink(block_node: Arc(BlockNode_101), is_full: false), balance: 800 },
    ]
}
```

**How it works**:
- Both empty and full variants reference **same Arc<BlockNode>**
- Each variant has its own `attesting_balance` (variant-specific weight)
- `BlockNode.total_weight` = empty.balance + full.balance = 1500

## Implementation Flow

### Flow 1: Empty Block Arrives
```rust
// In validate_block / insert_block:
let block_node = self.block_nodes
    .entry(block_root)
    .or_insert_with(|| Arc::new(BlockNode {
        block_root,
        block: block.clone_arc(),
        total_weight: RwLock::new(0),
    }))
    .clone();

let chain_link = ChainLink {
    block_node,
    block_state: Some(state),
    execution_state: None,
    is_full: false,
    // ... checkpoints ...
};

// Add to segment
segment.push(UnfinalizedBlock::new(chain_link));
unfinalized_locations.insert(block_root, location);
```

### Flow 2: Execution Payload Arrives
```rust
pub fn on_execution_payload(&mut self, envelope: ExecutionPayloadEnvelope) {
    let block_root = envelope.beacon_block_root;

    // 1. Get existing BlockNode (created when empty block arrived)
    let block_node = self.block_nodes
        .get(&block_root)
        .expect("BlockNode must exist from empty block")
        .clone();

    // 2. Get empty variant to copy checkpoints
    let empty_location = self.unfinalized_locations
        .get(&block_root)
        .expect("Empty variant must exist");
    let empty_chain_link = &self.unfinalized[&empty_location.segment_id][empty_location.position].chain_link;

    // 3. Process execution payload to get execution_state
    let execution_state = process_execution_payload(&envelope)?;

    // 4. Create full variant ChainLink
    let full_chain_link = ChainLink {
        block_node,  // SAME Arc as empty variant!
        block_state: None,
        execution_state: Some(execution_state),
        is_full: true,
        // Copy checkpoints from empty (same block, same chain context)
        current_justified_checkpoint: empty_chain_link.current_justified_checkpoint,
        finalized_checkpoint: empty_chain_link.finalized_checkpoint,
        unrealized_justified_checkpoint: empty_chain_link.unrealized_justified_checkpoint,
        unrealized_finalized_checkpoint: empty_chain_link.unrealized_finalized_checkpoint,
        payload_status: empty_chain_link.payload_status,
    };

    // 5. Add full variant to segment
    let full_block = UnfinalizedBlock::new(full_chain_link);
    self.unfinalized[&empty_location.segment_id].push(full_block);

    // 6. Track full variant location
    let full_position = self.unfinalized[&empty_location.segment_id].last_position();
    self.unfinalized_full_locations.insert(block_root, Location {
        segment_id: empty_location.segment_id,
        position: full_position,
    });
}
```

### Flow 3: Weight Propagation
**From Prysm**: Weights are propagated via parent-child pointers up the tree.

**Grandine Implementation**:
```rust
fn apply_balance_differences(&mut self, differences: ...) {
    for dissolved_difference in propagated_differences {
        let segment = &mut self.unfinalized[&segment_id];

        for unfinalized_block in segment.iter_mut_range(start..=end) {
            // Update variant-specific weight
            unfinalized_block.attesting_balance += difference;

            // Update total weight in shared BlockNode
            let mut total = unfinalized_block.chain_link.block_node.total_weight.write().unwrap();
            *total += difference;
        }
    }
}
```

**Key insight**: When iterating over segment, we hit BOTH empty and full variants, so `total_weight` automatically aggregates both.

### Flow 4: Head Selection
**From Prysm** (store.go:40-44):
```go
fullJustifiedNode, ok := s.fullNodeByPayload[justifiedNode.block.payloadHash]
if ok && fullJustifiedNode.weight >= justifiedNode.weight {
    justifiedNode = fullJustifiedNode  // Pick heavier variant
}
```

**Grandine Implementation** (to be added to update_head_segment_id):
```rust
fn update_head_segment_id(&mut self) {
    // For each block with both variants, compare weights
    for (block_root, empty_location) in &self.unfinalized_locations {
        if let Some(full_location) = self.unfinalized_full_locations.get(block_root) {
            let empty_block = &self.unfinalized[&empty_location.segment_id][empty_location.position];
            let full_block = &self.unfinalized[&full_location.segment_id][full_location.position];

            // Compare variant-specific weights
            if full_block.attesting_balance > empty_block.attesting_balance {
                // Full variant wins - use full_location for traversal
            } else {
                // Empty variant wins - use empty_location for traversal
            }
        }
    }

    // Continue with existing head selection logic using winner variants
}
```

**Alternative approach**: Use `BlockNode.total_weight` for comparison since it already aggregates both.

## What Prysm Taught Us

1. **Shared BlockNode**: Empty and full nodes share the same `BlockNode` struct via pointer
2. **Dual maps**: `emptyNodeByRoot[block_root]` and `fullNodeByPayload[payload_hash]` allow separate tracking
3. **Parent pointers**: `node.block.parent` - both empty and full reference the same parent BlockNode
4. **Weight comparison**: Head selection compares `fullNode.weight >= emptyNode.weight` to pick winner
5. **bestDescendant update**: When payload arrives, update all references to point to full variant if heavier

## Three Separate Concepts (Critical Distinction)

### 1. Engine API PayloadValidationStatus
```rust
pub enum PayloadValidationStatus {
    Valid,
    Invalid,
    Syncing,
    Accepted,
    InvalidBlockHash,
}
```
- From execution engine via `newPayload` RPC
- What the **EL says** about validity

### 2. Internal PayloadStatus (Optimistic Sync)
```rust
pub enum PayloadStatus {
    Valid,
    Invalid,
    Optimistic,
}
```
- Internal Grandine representation
- Stored in `ChainLink.payload_status`
- Used for fork viability checks

### 3. ePBS Payload Availability (Network/Fork Choice)
```rust
pub is_full: bool  // In ChainLink
```
- Has execution_state arrived via gossip?
- Affects fork choice weights
- **Independent of EL validation**

## Still TODO

1. **Implement `on_execution_payload` handler** - create full variant when payload arrives
2. **Update weight propagation** - ensure `BlockNode.total_weight` is updated
3. **Update head selection** - compare empty vs full variant weights
4. **PTC attestation processing** - check spec for PayloadAttestationData handling
5. **Fix all compilation errors** - update references to `chain_link.block_root` → `chain_link.block_node.block_root`

## References
- Prysm implementation: `/home/greendior/gran_pry/hello2/beacon-chain/forkchoice/doubly-linked-tree/`
- ePBS spec containers: `/home/greendior/gran_pry/hello/types/src/gloas/containers.rs`
- Grandine fork choice: `/home/greendior/gran_pry/hello/fork_choice_store/`
