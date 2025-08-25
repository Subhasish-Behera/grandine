# Rust Organization Hierarchy

## 📊 Complete Hierarchy Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                         WORKSPACE                                │
│                    (grandine_backup/)                            │
│                 [Contains Cargo.toml with                        │
│                  [workspace] section]                            │
│                                                                  │
│  ┌────────────────────────────────────────────────────────┐     │
│  │                      PACKAGE 1                          │     │
│  │                      (types/)                           │     │
│  │                 [Has its own Cargo.toml]                │     │
│  │                                                         │     │
│  │  ┌─────────────────────────────────────────────┐       │     │
│  │  │            LIBRARY CRATE                     │       │     │
│  │  │           (types/src/lib.rs)                 │       │     │
│  │  │         [Entry point for library]            │       │     │
│  │  │                                              │       │     │
│  │  │  ┌────────────────────────────────────┐     │       │     │
│  │  │  │         MODULE: phase0              │     │       │     │
│  │  │  │                                     │     │       │     │
│  │  │  │  ┌─────────────────────────┐       │     │       │     │
│  │  │  │  │  SUBMODULE: beacon_state │       │     │       │     │
│  │  │  │  └─────────────────────────┘       │     │       │     │
│  │  │  │  ┌─────────────────────────┐       │     │       │     │
│  │  │  │  │  SUBMODULE: containers  │       │     │       │     │
│  │  │  │  └─────────────────────────┘       │     │       │     │
│  │  │  └────────────────────────────────────┘     │       │     │
│  │  │                                              │       │     │
│  │  │  ┌────────────────────────────────────┐     │       │     │
│  │  │  │         MODULE: altair              │     │       │     │
│  │  │  └────────────────────────────────────┘     │       │     │
│  │  │                                              │       │     │
│  │  │  ┌────────────────────────────────────┐     │       │     │
│  │  │  │         MODULE: config              │     │       │     │
│  │  │  └────────────────────────────────────┘     │       │     │
│  │  └─────────────────────────────────────────────┘       │     │
│  └────────────────────────────────────────────────────────┘     │
│                                                                  │
│  ┌────────────────────────────────────────────────────────┐     │
│  │                      PACKAGE 2                          │     │
│  │                     (grandine/)                         │     │
│  │                 [Has its own Cargo.toml]                │     │
│  │                                                         │     │
│  │  ┌─────────────────────────────────────────────┐       │     │
│  │  │            BINARY CRATE                      │       │     │
│  │  │         (grandine/src/main.rs)               │       │     │
│  │  │      [Has main() function - executable]      │       │     │
│  │  │                                              │       │     │
│  │  │  ┌────────────────────────────────────┐     │       │     │
│  │  │  │      MODULE: commands               │     │       │     │
│  │  │  └────────────────────────────────────┘     │       │     │
│  │  │  ┌────────────────────────────────────┐     │       │     │
│  │  │  │      MODULE: validators             │     │       │     │
│  │  │  └────────────────────────────────────┘     │       │     │
│  │  └─────────────────────────────────────────────┘       │     │
│  └────────────────────────────────────────────────────────┘     │
│                                                                  │
│  ┌────────────────────────────────────────────────────────┐     │
│  │                      PACKAGE 3                          │     │
│  │                        (bls/)                           │     │
│  │                 [Has its own Cargo.toml]                │     │
│  │                                                         │     │
│  │  ┌─────────────────────────────────────────────┐       │     │
│  │  │            LIBRARY CRATE                     │       │     │
│  │  │            (bls/src/lib.rs)                  │       │     │
│  │  └─────────────────────────────────────────────┘       │     │
│  │                                                         │     │
│  │  ┌────────────────────────────────────────────────┐    │     │
│  │  │           SUB-PACKAGE: bls-blst                 │    │     │
│  │  │          (bls/bls-blst/Cargo.toml)              │    │     │
│  │  │  ┌─────────────────────────────────────────┐   │    │     │
│  │  │  │        LIBRARY CRATE                     │   │    │     │
│  │  │  │     (bls/bls-blst/src/lib.rs)            │   │    │     │
│  │  │  └─────────────────────────────────────────┘   │    │     │
│  │  └────────────────────────────────────────────────┘    │     │
│  └────────────────────────────────────────────────────────┘     │
│                                                                  │
│                    [...64 more packages...]                      │
└─────────────────────────────────────────────────────────────────┘
```

## 🎯 Simplified Hierarchy (Top to Bottom)

```
                          WORKSPACE
                              │
                              ├── Cargo.toml (workspace config)
                              ├── Cargo.lock (shared deps)
                              │
        ┌─────────────────────┼─────────────────────┐
        │                     │                     │
    PACKAGE 1            PACKAGE 2            PACKAGE 3
    (types/)            (grandine/)            (bls/)
        │                     │                     │
        ├── Cargo.toml        ├── Cargo.toml        ├── Cargo.toml
        │                     │                     │
   LIBRARY CRATE         BINARY CRATE         LIBRARY CRATE
   (src/lib.rs)         (src/main.rs)         (src/lib.rs)
        │                     │                     │
   ┌────┴────┐           ┌────┴────┐           ┌───┴────┐
   │         │           │         │           │        │
MODULE   MODULE      MODULE    MODULE      MODULE   SUB-PACKAGE
(phase0) (altair)  (commands) (validators)          (bls-blst)
   │                                                     │
   ├── beacon_state                              LIBRARY CRATE
   ├── containers                                 (src/lib.rs)
   └── primitives                                        │
                                                    ┌────┴────┐
                                                    │         │
                                                 MODULE   MODULE
```

## 🔄 Relationship Hierarchy

```
WORKSPACE
    ↓ contains
PACKAGES (multiple)
    ↓ contains
CRATES (1-2 per package typically)
    ↓ types of crates
    ├── BINARY (executable, has main())
    └── LIBRARY (reusable code, no main())
         ↓ contains
    MODULES (organize code within crate)
         ↓ can contain
    SUBMODULES (nested organization)
```

## 📦 Size & Scope Comparison

```
┌──────────────────────────────────────────────────────┐
│ WORKSPACE         🏢 Entire Project                  │
│                   67 packages in Grandine            │
│                                                      │
│  ┌───────────────────────────────────────────┐      │
│  │ PACKAGE       📦 Directory with Cargo.toml │      │
│  │               e.g., types/, grandine/, bls/│      │
│  │                                            │      │
│  │  ┌──────────────────────────────────┐     │      │
│  │  │ CRATE    📚/🎯 Compilation unit   │     │      │
│  │  │          lib.rs or main.rs        │     │      │
│  │  │                                   │     │      │
│  │  │  ┌───────────────────────┐       │     │      │
│  │  │  │ MODULE 📁 Code grouping│       │     │      │
│  │  │  │       mod phase0 {}    │       │     │      │
│  │  │  │                        │       │     │      │
│  │  │  │  ┌──────────────┐     │       │     │      │
│  │  │  │  │ SUBMODULE 📄 │     │       │     │      │
│  │  │  │  │ beacon_state │     │       │     │      │
│  │  │  │  └──────────────┘     │       │     │      │
│  │  │  └───────────────────────┘       │     │      │
│  │  └──────────────────────────────────┘     │      │
│  └───────────────────────────────────────────┘      │
└──────────────────────────────────────────────────────┘

LARGEST ←────────────────────────────────→ SMALLEST
```

## 🔑 Key Properties at Each Level

| Level | Has Cargo.toml | Can be Compiled | Can be Executed | Can be Imported |
|-------|----------------|-----------------|-----------------|-----------------|
| **Workspace** | ✅ (root) | ✅ (all packages) | Depends | ❌ |
| **Package** | ✅ | ✅ | Depends on crate type | ✅ (if library) |
| **Binary Crate** | ❌ (uses package's) | ✅ | ✅ | ❌ |
| **Library Crate** | ❌ (uses package's) | ✅ | ❌ | ✅ |
| **Module** | ❌ | ❌ (part of crate) | ❌ | ✅ |

## 🎨 Visual Memory Aid

```
Think of it as a City:

🏙️ WORKSPACE = Entire City (Grandine)
   │
   ├── 🏢 PACKAGE = Building (types building)
   │     │
   │     ├── 🏗️ BINARY CRATE = Main Entrance/Lobby (can enter here)
   │     │     └── Has main() function, you start here
   │     │
   │     └── 📚 LIBRARY CRATE = Shared Facilities (gym, pool)
   │           └── Used by residents, not an entrance
   │
   ├── 🏢 PACKAGE = Another Building (grandine building)
   │     │
   │     └── 🎯 BINARY CRATE = Another Entrance
   │
   └── 🏢 PACKAGE = Another Building (bls building)
         │
         └── 📚 LIBRARY CRATE = More Shared Facilities

Within each building (crate):
   📁 MODULE = Floor (phase0 floor)
      📄 SUBMODULE = Room (beacon_state room)
```

## 🔗 Dependency Flow

```
                    grandine (BINARY)
                    src/main.rs
                         │
                    ┌────┴────┐
                    │ imports │
                    └────┬────┘
                         ↓
        ┌────────────────┼────────────────┐
        ↓                                  ↓
   types (LIBRARY)                   bls (LIBRARY)
   src/lib.rs                        src/lib.rs
        │                                  │
   ┌────┴────┐                        ┌───┴────┐
   │ exports │                        │ exports│
   └────┬────┘                        └───┬────┘
        ↓                                  ↓
   - phase0 module                    - PublicKey
   - altair module                    - Signature
   - config module                    - SecretKey
```

This hierarchy shows that:
1. **Workspace** is the container for everything
2. **Packages** are independent units with their own dependencies
3. **Crates** are what actually gets compiled
4. **Binary crates** are entry points (executables)
5. **Library crates** provide reusable functionality
6. **Modules** organize code within crates