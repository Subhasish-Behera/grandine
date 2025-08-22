# Understanding Grandine's Collections: Why It's a Smart Optimization

## The Problem: State Storage in Ethereum

In Ethereum consensus, the BeaconState is massive and changes every slot (12 seconds). A naive approach would:
1. Copy the entire state for each slot
2. Store each state separately
3. Use tons of memory and disk space

## The Solution: Persistent Data Structures

Grandine uses **persistent (immutable) data structures** that share memory between versions. Think of it like Git:
- Git doesn't copy your entire codebase for each commit
- It only stores the *differences*
- Multiple commits can share unchanged files

## How Collections.rs Works

```rust
// Regular Vec: Each state has its own copy
State1: [validator1, validator2, validator3, ...]  // 1GB
State2: [validator1, validator2, validator3_modified, ...]  // Another 1GB!

// PersistentList: States share unchanged data
State1: → [validator1] → [validator2] → [validator3] → ...
           ↑              ↑              ↑
State2: ───┘              └──────────────┘   [validator3_modified] → ...
// Only the modified validator is duplicated!
```

## Two Types of Collections

### 1. **Persistent Collections** (in BeaconState)
```rust
pub type Validators<P> = PersistentList<Validator, P::ValidatorRegistryLimit>;
pub type Balances<P> = PersistentList<Gwei, P::ValidatorRegistryLimit>;
```
- Used for data that persists across states
- Enables memory sharing between consecutive states
- Perfect for validators, balances, etc. that rarely all change at once

### 2. **Contiguous Collections** (in BeaconBlockBody)
```rust
pub attestations: ContiguousList<Attestation<P>, P::MaxAttestations>
```
- Used for temporary data in blocks
- Simple, fast, doesn't need persistence
- Gets processed and discarded

## Why This Matters for ePBS

When we add ePBS fields to BeaconState:
```rust
// These need persistent collections because they're part of state
pub builder_pending_payments: BuilderPendingPayments<P>,  // PersistentVector
pub builder_pending_withdrawals: BuilderPendingWithdrawals<P>,  // PersistentList
```

But in BeaconBlockBody:
```rust
// These use ContiguousList because they're temporary
pub payload_attestations: ContiguousList<PayloadAttestation<P>, P::MaxPayloadAttestations>
```

## The Performance Impact

### Without Persistent Collections:
- Storing 100 states × 500MB each = 50GB memory
- Each state transition copies everything

### With Persistent Collections:
- Storing 100 states might only use 2-3GB
- State transitions only copy what changes
- Can keep many states in memory for fast access

## Real-World Example

Imagine tracking 1 million validators:
1. **Block arrives** with a few slashings
2. **Without persistence**: Copy all 1M validators, modify 3
3. **With persistence**: Share 999,997 validators, only store 3 new ones

## Bundle Sizes (The UnhashedBundleSize)

```rust
pub type Balances<P> = 
    PersistentList<Gwei, P::ValidatorRegistryLimit, UnhashedBundleSize<Gwei>>;
```

The `UnhashedBundleSize` controls how many items are grouped together:
- Larger bundles = less rehashing, more memory
- Smaller bundles = more rehashing, less memory
- Grandine chose larger bundles for speed

## Summary

Collections.rs isn't just an optimization—it's **essential** for:
1. Running a validator that tracks multiple states efficiently
2. Fast fork choice (need many states in memory)
3. Serving historical states without massive storage
4. Quick state transitions (only update what changed)

Think of it like this: Without persistent collections, Grandine would need server-grade hardware. With them, it can run on reasonable machines while maintaining excellent performance.

This is why Grandine is known for being memory-efficient compared to other clients!