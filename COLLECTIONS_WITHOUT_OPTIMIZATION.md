# What If We DON'T Use Collections.rs?

## Option 1: Direct Vec/ContiguousList in BeaconState (No Optimization)

Instead of:
```rust
// In collections.rs
pub type BuilderPendingPayments<P> = 
    PersistentVector<BuilderPendingPayment, P::MaxBuilderPendingPayments>;

// In beacon_state.rs
pub builder_pending_payments: BuilderPendingPayments<P>,
```

We could just write:
```rust
// Directly in beacon_state.rs - NO MEMORY SHARING!
pub builder_pending_payments: Vec<BuilderPendingPayment>,
// or
pub builder_pending_payments: ContiguousList<BuilderPendingPayment, P::MaxBuilderPendingPayments>,
```

## What Would Break?

### 1. Memory Explosion
```rust
// Scenario: 100 beacon states in memory, each with 1000 pending payments

// WITH PersistentVector (current):
State1: [payment1, payment2, ..., payment1000]  // 1MB
State2: shares 999 payments, only payment500 changed  // +1KB
State3: shares 998 payments, two more changed  // +2KB
Total: ~1.1MB

// WITHOUT PersistentVector (using Vec):
State1: [payment1, payment2, ..., payment1000]  // 1MB
State2: [payment1, payment2, ..., payment500_modified, ...]  // Another 1MB!
State3: [payment1, payment2, ..., payment500_modified, ...]  // Another 1MB!
Total: 100MB!
```

### 2. State Cloning Performance
```rust
// WITH PersistentVector:
fn clone_state(&self) -> BeaconState {
    BeaconState {
        builder_pending_payments: self.builder_pending_payments.clone(),  // Just copies pointer + refcount
        // ... (almost instant)
    }
}

// WITHOUT (using Vec):
fn clone_state(&self) -> BeaconState {
    BeaconState {
        builder_pending_payments: self.builder_pending_payments.clone(),  // Copies entire Vec!
        // ... (could take milliseconds for large vectors)
    }
}
```

### 3. Fork Choice Would Slow Down
```rust
// Fork choice needs multiple states in memory:
// WITH optimization: Can keep 1000+ states in RAM
// WITHOUT: Maybe only 10-20 states before OOM
```

## The "Unoptimized" Version Would Look Like:

```rust
// eip7732/beacon_state.rs - WITHOUT collections.rs
use ssz::ContiguousList;  // Simple list, no sharing

pub struct BeaconState<P: Preset> {
    // ... other fields ...
    
    // These would work but perform terribly:
    pub builder_pending_payments: ContiguousList<BuilderPendingPayment, P::MaxBuilderPendingPayments>,
    pub builder_pending_withdrawals: ContiguousList<BuilderPendingWithdrawal, P::BuilderPendingWithdrawalsLimit>,
    
    // Every state clone would duplicate these entire lists!
}
```

## Why Grandine MUST Use Collections

It's not really optional because:

1. **Validators need multiple states**: For attestations, fork choice, etc.
2. **Serving API requests**: Need historical states readily available
3. **Reorgs**: Need to quickly switch between different state branches

Without persistent collections, Grandine would need:
- 10-100x more RAM
- Much faster CPU (for all the copying)
- Wouldn't be competitive with other clients

## Could We Make It Optional?

Theoretically, yes:
```rust
// A "simple mode" for testing
#[cfg(feature = "no-persistence")]
pub type BuilderPendingPayments<P> = Vec<BuilderPendingPayment>;

#[cfg(not(feature = "no-persistence"))]
pub type BuilderPendingPayments<P> = 
    PersistentVector<BuilderPendingPayment, P::MaxBuilderPendingPayments>;
```

But in practice, this would only be useful for:
- Unit tests
- Spec test vectors
- Never for production

## The Real Alternative: Different Client Architecture

Other clients like Prysm use different strategies:
- **Prysm**: Uses protobuf, different state management
- **Lighthouse**: Uses tree-hashing caches differently
- **Nimbus**: Written in Nim with different memory model

Each has trade-offs, but ALL optimize state storage somehow!

## Summary

You COULD skip collections.rs by using:
```rust
// In beacon_state.rs directly
pub builder_pending_payments: Vec<BuilderPendingPayment>,
```

But this would:
- 🔴 Use 10-100x more memory
- 🔴 Make state clones slow
- 🔴 Make Grandine uncompetitive
- 🔴 Require server-grade hardware for basic validation

So while technically "optional", it's practically mandatory for a production consensus client. The "unoptimized" version would only work for toy implementations or testing.