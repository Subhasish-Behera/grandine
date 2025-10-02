# Fork Choice Implementation Analysis: Grandine vs Teku

## Table of Contents
1. [Fundamental Data Structure Differences](#1-fundamental-data-structure-differences)
2. [The Segment Structure Deep Dive](#2-the-segment-structure-deep-dive)
3. [ChainLink and UnfinalizedBlock Architecture](#3-chainlink-and-unfinalizedblock-architecture)
4. [LMD-GHOST Weight Calculation](#4-lmd-ghost-weight-calculation)
5. [Casper FFG and Epoch-Based Processing](#5-casper-ffg-and-epoch-based-processing)
6. [Fork Choice for All Nodes](#6-fork-choice-for-all-nodes)
7. [Performance and Architectural Trade-offs](#7-performance-and-architectural-tradeoffs)

---

## 1. Fundamental Data Structure Differences

### Teku: Tree of Individual Nodes

Teku uses a **pure tree structure** where each block is an independent node with explicit parent-child relationships.

```java
class ProtoNode {
    // Identity
    Bytes32 blockRoot;        // 32 bytes - just a hash
    Bytes32 stateRoot;        // 32 bytes - just a hash  
    UInt64 blockSlot;         // 8 bytes
    
    // Tree relationships (indices into array)
    Optional<Integer> parentIndex;      // My parent
    Optional<Integer> bestChildIndex;   // My best child
    Optional<Integer> bestDescendantIndex; // Best in my subtree
    
    // LMD-GHOST weight
    UInt64 weight;            // 8 bytes
    
    // NO ACTUAL BLOCK DATA!
}

// Storage: Flat array with index lookups
ProtoNode[] nodes = [node0, node1, node2, ...];
HashMap<BlockRoot, Integer> rootToIndex;
```

**Visual representation:**
```
Individual Nodes with Parent-Child Pointers:

        [Block A]
           / \
          /   \
    [Block B] [Block C]
        |       / \
    [Block D] [E] [F]
        |
    [Block G]

Stored as flat array with indices:
Index: [0]    [1]    [2]    [3]    [4]    [5]    [6]
Block: [A]    [B]    [C]    [D]    [E]    [F]    [G]
Parent: null   0      0      1      2      2      3
Weight: 100    60     40     50     20     20     50
```

### Grandine: Segment-Based Chain-Links

Grandine uses a **segment-based approach** where linear chains of blocks are grouped together.

```rust
struct Store<P> {
    // Only TWO buckets:
    finalized: Vector<ChainLink<P>>,              // Bucket 1: Finalized
    unfinalized: OrdMap<SegmentId, Segment<P>>,   // Bucket 2: Everything else!
}

struct Segment<P> {
    blocks: Vector<UnfinalizedBlock<P>>,  // Linear sequence
    first_position: Position,             // Tracks finalized blocks removed
}

struct UnfinalizedBlock<P> {
    chain_link: ChainLink<P>,      // Full block data
    attesting_balance: Gwei,       // Fork choice weight
}
```

**Visual representation:**
```
Linear segments with branch points:

Segment 1: [A]
            |
Segment 2: [B → D → G]  (linear chain)
            |
Segment 3: [C → E]      (linear chain)
            |
Segment 4: [C → F]      (branches from C)

Visualized:
        [A]
      ___↓___
     /        \
[B→D→G]      [C]
           ___↓___
          /       \
        [E]       [F]
```

### Why Segments?

The key insight: **Most blockchain forks are short-lived**. The chain is mostly linear with occasional branches!

```
Typical blockchain (99% of the time):
[Genesis] → [1] → [2] → [3] → [4] → [5] → [6] → [7] → [8] → [9] → [10] → ...
                                      ↓
                                   [5'] (orphaned)

Not a bushy tree but a "vine with occasional leaves"!
```

---

## 2. The Segment Structure Deep Dive

### The first_position Field

The `first_position` field is a clever optimization for handling finalization:

```rust
struct Segment<P> {
    blocks: Vector<UnfinalizedBlock<P>>,
    first_position: Position,  // Tracks how many blocks were finalized and removed
}
```

#### Example: Finalization Process

**Step 1: Initial Segment**
```
Segment {
    blocks: [A, B, C, D, E, F],
    first_position: 0,
}

Positions:  [0] [1] [2] [3] [4] [5]
Blocks:     [A] [B] [C] [D] [E] [F]
```

**Step 2: Block A Gets Finalized**
```
Segment {
    blocks: [B, C, D, E, F],
    first_position: 1,  // Remembers A was removed!
}

Logical positions: [1] [2] [3] [4] [5]
Actual array:      [0] [1] [2] [3] [4]
Blocks:            [B] [C] [D] [E] [F]
```

**Step 3: Blocks B and C Get Finalized**
```
Segment {
    blocks: [D, E, F],
    first_position: 3,  // Remembers 3 blocks were removed
}

Logical positions: [3] [4] [5]
Actual array:      [0] [1] [2]
Blocks:            [D] [E] [F]
```

This allows O(1) finalization without shifting arrays!

### Why SegmentId is External

The `SegmentId` is not stored in the Segment itself:

```rust
// Store maintains the mapping:
unfinalized: OrdMap<SegmentId, Segment<P>>,  // ID → Segment

// Segment doesn't know its own ID:
struct Segment<P> {
    blocks: Vector<UnfinalizedBlock<P>>,
    first_position: Position,
    // NO segment_id field!
}
```

This design allows clean segment splits and merges during reorganizations.

---

## 3. ChainLink and UnfinalizedBlock Architecture

### The Three-Layer Structure

```rust
// Layer 1: The immutable block data and state
struct ChainLink<P> {
    block_root: H256,
    block: Arc<SignedBeaconBlock<P>>,
    state: Option<Arc<BeaconState<P>>>,
    
    // Checkpoints snapshot at block arrival
    current_justified_checkpoint: Checkpoint,
    finalized_checkpoint: Checkpoint,
    unrealized_justified_checkpoint: Checkpoint,
    unrealized_finalized_checkpoint: Checkpoint,
    
    payload_status: PayloadStatus,
}

// Layer 2: Fork choice wrapper (THE NODE!)
struct UnfinalizedBlock<P> {
    pub chain_link: ChainLink<P>,      // Immutable data
    pub attesting_balance: Gwei,       // Mutable fork choice weight
}

// Layer 3: Container structure
struct Segment<P> {
    blocks: Vector<UnfinalizedBlock<P>>,
}
```

### The Journey of a Block

```
New block arrives via network
    ↓
Create ChainLink (immutable data + checkpoints)
    ↓
Wrap in UnfinalizedBlock (adds mutable fork choice data)
    ↓
Insert into appropriate Segment
```

### Why This Design?

**UnfinalizedBlock** is Grandine's equivalent of Teku's ProtoNode:

| Aspect | Teku ProtoNode | Grandine UnfinalizedBlock |
|--------|----------------|---------------------------|
| **Contains** | Just references (hashes) | Full block data via ChainLink |
| **Weight** | `UInt64 weight` | `attesting_balance: Gwei` |
| **Tree structure** | Explicit parent/child indices | Implicit via segment position |
| **Memory** | ~52 bytes overhead | Full block embedded |

The naming "UnfinalizedBlock" reflects **storage location**, not block state:
- ✅ "Block stored in the unfinalized section"
- ❌ NOT "Block that isn't finalized yet" (though that's also true)

---

## 4. LMD-GHOST Weight Calculation

### Teku's Approach: Tree Propagation

```java
void applyScoreChanges(deltas) {
    // 1. Apply deltas to each node
    for (node : nodes) {
        node.adjustWeight(delta);
        // Propagate to parent
        parent.delta += node.delta;
    }
    // 2. Update best child/descendant
    updateBestDescendants();
}
```

Weight changes propagate up the tree node-by-node through parent indices.

### Grandine's Approach: Direct Application

```rust
fn apply_attestation(&mut self, attestation: Attestation) {
    // Find the UnfinalizedBlock (the "node")
    let unfinalized_block = find_block(attestation.target);
    
    // Update fork choice weight directly (no propagation!)
    unfinalized_block.attesting_balance += attestation.weight;
}

fn score(&self, unfinalized_block: &UnfinalizedBlock) -> Score {
    let attestation_score = unfinalized_block.attesting_balance;
    let proposer_boost = if is_ancestor_of_boost_root {
        self.timely_proposer_score
    } else { 0 };
    Score { attestation_score, proposer_boost, tiebreaker }
}
```

### The Double-Loop Pattern in Grandine

```rust
// Finding the head (best block):
fn find_head(&self) -> &UnfinalizedBlock {
    self.unfinalized
        .values()                              // OUTER LOOP: All segments
        .flat_map(|segment| segment.blocks)    // INNER LOOP: All blocks in segment
        .filter(|block| !block.is_invalid())
        .max_by_key(|block| block.attesting_balance)
        .unwrap()
}
```

### Performance Comparison

| Operation | Teku | Grandine |
|-----------|------|----------|
| **Process attestation** | O(depth) propagation | O(log blocks_in_segment) |
| **Find head** | O(depth) traversal | O(segments × blocks/segment) |
| **Memory per block** | ~52 bytes overhead | ~8 bytes overhead |

---

## 5. Casper FFG and Epoch-Based Processing

### The Timing Dichotomy

**LMD-GHOST (Every slot/attestation):**
- Updates continuously
- Processes attestations immediately
- Recalculates head frequently
- Real-time decision making

**Casper FFG (Epoch boundaries only):**
- Batch processing every 32 slots
- Counts all attestations from epoch
- Applies k-finality rule
- Updates justified/finalized once per epoch

### The Unrealized Checkpoints Mechanism

#### Phase 1: Block Arrival - Compute Unrealized
```rust
// store.rs:1393-1406 - When validating block
let (unrealized_justified_checkpoint, unrealized_finalized_checkpoint) = {
    let mut state = state.clone_arc();
    // Pull up to next epoch boundary
    combined::process_justification_and_finalization(state.make_mut())?;
    (state.current_justified_checkpoint(), state.finalized_checkpoint())
};
```

#### Phase 2: Store in ChainLink
```rust
let chain_link = ChainLink {
    // Current view (what block claims)
    current_justified_checkpoint: justified_checkpoint,
    finalized_checkpoint,
    
    // Future view (pre-computed!)
    unrealized_justified_checkpoint,
    unrealized_finalized_checkpoint,
};
```

#### Phase 3: Epoch Boundary - "Pull Up"
```rust
// store.rs:2011-2021 - THE KEY MOMENT
if is_new_epoch {
    // Pull up unrealized to realized!
    self.update_checkpoints(
        self.unrealized_justified_checkpoint,  // Becomes justified
        self.unrealized_finalized_checkpoint,  // Becomes finalized
    );
    
    if finalized_checkpoint_updated {
        self.prune_after_finalization();  // Clean up
    }
}
```

### Timeline Example
```
Slot 30: Block arrives
    ↓
    Compute unrealized checkpoints (expensive)
    Store in ChainLink
    
Slot 31: Another block
    ↓
    Same process
    
Slot 32: EPOCH BOUNDARY
    ↓
    apply_tick() detects new epoch
    ↓
    justified = unrealized_justified (cheap!)
    finalized = unrealized_finalized (cheap!)
    ↓
    Trigger finalization actions
```

### Why Store Doesn't Compute Casper FFG

The k-finality rule lives in state transitions, not Store:

```rust
// Simplified from process_justification_and_finalization
if current_justified.epoch == previous_justified.epoch + 1 {
    // Consecutive epochs justified!
    state.finalized_checkpoint = previous_justified;  // k-finality!
}
```

Store just receives and applies pre-computed results!

---

## 6. Fork Choice for All Nodes

### The Universal Requirement

**ALL nodes run fork choice**, not just validators!

#### Grandine's Implementation
```rust
// runtime.rs:256 - ALWAYS executed regardless of validator_enabled
let (controller, mutator_handle) = Controller::new(
    chain_config.clone_arc(),
    pubkey_cache.clone_arc(),
    store_config,
    anchor_block,
    anchor_state.clone_arc(),
    current_tick,
    // ... channels for various services ...
)?;

// runtime.rs:760-775 - Clock updates Controller every slot
async fn run_clock<P: Preset>(
    controller: RealController<P>,
) -> Result<()> {
    loop {
        tick = ticks.select_next_some() => {
            controller.on_tick(tick?);  // Updates fork choice!
        }
    }
}
```

### Why Non-Validators Need Fork Choice

1. **To know which chain to follow** - Multiple forks may exist
2. **To validate incoming blocks** - Is this block on the canonical chain?
3. **To serve correct data** - RPC nodes need to know the head
4. **Network consensus** - Everyone must agree on the canonical chain

### The Difference

| Node Type | Fork Choice Operations |
|-----------|------------------------|
| **All Nodes** | Track canonical chain, process attestations/blocks from network, update weights |
| **Validators Only** | ALSO produce blocks/attestations based on fork choice |

```rust
// Non-validator:
let head = store.find_head();  // YES - needs fork choice
let attestation = create_attestation(head);  // NO - can't create

// Validator:
let head = store.find_head();  // YES - needs fork choice
let attestation = create_attestation(head);  // YES - can create
```

---

## 7. Performance and Architectural Trade-offs

### Memory Efficiency

**Teku:**
```
Per block:
- ProtoNode overhead: ~52 bytes
- For 10,000 blocks: 520KB overhead
- Blocks stored separately
```

**Grandine:**
```
Per block:
- UnfinalizedBlock overhead: ~8 bytes
- For 10,000 blocks: 80KB overhead (6x less!)
- Blocks embedded in segments
```

### Operation Complexity

| Operation | Teku | Grandine |
|-----------|------|----------|
| **Add block** | O(1) + O(depth) propagation | O(1) usually |
| **Find head** | O(depth) | O(segments × blocks/segment) |
| **Update weight** | O(depth) per attestation | O(log blocks/segment) |
| **Reorg** | O(depth) pointer updates | O(segments) score updates |
| **Cache efficiency** | Poor (pointer chasing) | Good (sequential access) |

### Real Example: Processing 1000 Attestations

**Teku:**
```
for attestation in attestations:
    block.weight += attestation.weight      # O(1)
    # Propagate up the tree
    while current.parent exists:            # O(depth=100)
        parent.delta += attestation.weight
        
Total: 1000 × O(100) = O(100,000) operations
```

**Grandine:**
```
for segment, attestations in attestations_by_segment:
    for attestation in attestations:
        block.attesting_balance += weight   # O(1)
        # No propagation!

Total: O(1000 × log(10)) ≈ O(3,000) operations (33x faster!)
```

### The Architectural Trade-off

**Teku (Tree/Separation):**
- ✅ Flexible - blocks independent of tree
- ✅ Deduplication - block stored once
- ✅ Simple conceptually
- ❌ Cache misses
- ❌ Multiple data structures

**Grandine (Segments/Embedding):**
- ✅ Cache locality
- ✅ Single structure
- ✅ Memory efficient
- ❌ Potential duplication
- ❌ Complex reorganizations

### Why Both Work Well

Fork choice is **not the bottleneck**. The real costs are:
1. **State transitions** (100ms+)
2. **Signature verification** (10ms+)
3. **Network I/O** (50ms+)
4. **Database writes** (20ms+)

Fork choice updates (1-2ms) are negligible, so both architectures work fine!

---

## Summary

The fundamental difference is architectural philosophy:

- **Teku**: General-purpose tree structure treating all cases equally
- **Grandine**: Optimized for the common case (linear chains with occasional forks)

Both implement the same LMD-GHOST + Casper FFG algorithms but with different data structure optimizations. Grandine's segment-based approach with embedded data provides better cache locality and memory efficiency, while Teku's tree structure offers more flexibility and cleaner separation of concerns.

The key insights:
1. Fork choice runs on ALL nodes, not just validators
2. Casper FFG is computed with blocks but applied at epoch boundaries
3. The "unrealized → realized" checkpoint mechanism bridges continuous LMD-GHOST with periodic Casper FFG
4. Performance differences are overshadowed by other bottlenecks (state transitions, networking)
5. Both architectures successfully implement the same consensus rules with different trade-offs