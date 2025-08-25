# Rust Code Organization: From Small to Large

## 📚 Core Concepts Overview

| Concept | What It Is | Can Execute? | Has Cargo.toml? | Example in Grandine |
|---------|------------|--------------|-----------------|---------------------|
| **Module** | Code grouping within a file | ❌ | ❌ | `pub mod phase0` |
| **Crate** | Compilation unit | Depends | ❌ | `lib.rs` or `main.rs` |
| **Library** | Reusable code crate | ❌ | ❌ | `types/src/lib.rs` |
| **Binary** | Executable crate | ✅ | ❌ | `grandine/src/main.rs` |
| **Package** | Crate(s) + Cargo.toml | Depends | ✅ | `types/` directory |
| **Workspace** | Multiple packages | Depends | ✅ | Entire Grandine project |

## 🔍 Detailed Explanations with Examples

### 1. Module (smallest unit)
**What**: A namespace for grouping related code  
**Where**: Inside `.rs` files or as separate files/directories  
**Purpose**: Organization and privacy control

```rust
// Simple inline module
mod math {
    pub fn add(a: i32, b: i32) -> i32 { a + b }
    
    mod private {  // Nested private module
        fn helper() {}
    }
}
```

**Grandine Example** (`types/src/lib.rs`):
```rust
pub mod phase0 {           // Public module
    pub mod beacon_state;   // Nested public submodule
    mod container_impls;    // Nested private submodule
}
```

### 2. Crate (compilation unit)
**What**: What the Rust compiler actually compiles  
**Where**: Starts from `lib.rs` (library) or `main.rs` (binary)  
**Purpose**: The atomic unit of compilation

```
types/src/
├── lib.rs        # Root of the 'types' library crate
├── cache.rs      # Part of the crate
└── config.rs     # Part of the crate
```

### 3. Library vs Binary

#### Library Crate
**What**: Code meant to be used by other crates  
**Entry**: `src/lib.rs`  
**Can Run**: ❌ No `main()` function

**Grandine Example** (`types/src/lib.rs`):
```rust
// Library crate - exposes functionality
pub mod cache;
pub mod config;
pub mod traits;
// No main() - can't run directly
```

#### Binary Crate  
**What**: An executable program  
**Entry**: `src/main.rs` or `src/bin/*.rs`  
**Can Run**: ✅ Has `main()` function

**Grandine Example** (`grandine/src/main.rs`):
```rust
use types::config::Config;  // Uses library crate

fn main() {
    println!("I can be executed!");
    // Application starts here
}
```

### 4. Package (crate container)
**What**: A bundle of one or more crates  
**Where**: Directory with `Cargo.toml`  
**Purpose**: Define dependencies, metadata, build configuration

```
types/                    # A PACKAGE
├── Cargo.toml           # Package manifest
├── src/
│   ├── lib.rs          # Library crate (optional)
│   ├── main.rs         # Binary crate (optional)
│   └── bin/
│       └── tool.rs     # Additional binary crate
```

**Grandine Package Structure**:
```toml
# types/Cargo.toml
[package]
name = 'types'
authors = ["Grandine <info@grandine.io>"]

[dependencies]
arithmetic = { workspace = true }  # Uses workspace dependency
bls = { workspace = true }
```

### 5. Workspace (the big picture)
**What**: Collection of related packages that share dependencies  
**Where**: Root directory with `[workspace]` in Cargo.toml  
**Purpose**: Manage multiple packages together, share `Cargo.lock`

**Grandine Workspace** (`/Cargo.toml`):
```toml
[workspace]
members = [
    'types',        # Package 1: Type definitions library
    'grandine',     # Package 2: Main executable
    'bls',          # Package 3: Cryptography library
    # ... 64 more packages
]

[workspace.dependencies]
# Shared dependencies for all packages
anyhow = { version = '1', features = ['backtrace'] }
serde = { version = '1', features = ['derive', 'rc'] }
```

## 🏗️ How Grandine Is Structured

```
grandine_backup/                        🏢 WORKSPACE
│
├── Cargo.toml                         [workspace] definition
├── Cargo.lock                         Shared dependency lock
│
├── types/                             📦 PACKAGE: Type definitions
│   ├── Cargo.toml                     
│   └── src/
│       ├── lib.rs                     📚 LIBRARY CRATE root
│       ├── cache.rs                   📄 MODULE (auto from file)
│       ├── config.rs                  📄 MODULE
│       └── phase0/                    📁 MODULE (directory)
│           ├── mod.rs                 Module definition
│           ├── beacon_state.rs        📄 SUBMODULE
│           └── containers.rs          📄 SUBMODULE
│
├── grandine/                          📦 PACKAGE: Main application  
│   ├── Cargo.toml
│   └── src/
│       └── main.rs                    🎯 BINARY CRATE (executable)
│
├── bls/                               📦 PACKAGE: Cryptography
│   ├── Cargo.toml
│   ├── src/
│   │   └── lib.rs                    📚 LIBRARY CRATE
│   │
│   ├── bls-blst/                     📦 SUB-PACKAGE
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   │
│   └── bls-zkcrypto/                 📦 SUB-PACKAGE
│       ├── Cargo.toml
│       └── src/lib.rs
│
└── [64 more packages...]

```

## 🔗 How They Connect

### 1. Workspace → Package
```toml
# Root Cargo.toml
[workspace]
members = ["types", "grandine", "bls"]

[workspace.dependencies]
serde = "1.0"  # All packages can use this
```

### 2. Package → Package
```toml
# grandine/Cargo.toml
[dependencies]
types = { path = "../types" }      # Local package
bls = { path = "../bls" }          # Local package
serde = { workspace = true }       # From workspace
```

### 3. Binary uses Libraries
```rust
// grandine/src/main.rs
use types::config::Config;         // From types package
use bls::PublicKey;               // From bls package

fn main() {
    let config = Config::default();
    // Run the application
}
```

### 4. Module Hierarchy
```rust
// To use nested modules:
use types::phase0::beacon_state::BeaconState;
//   │      │         │
//   │      │         └── submodule
//   │      └── module  
//   └── crate/package
```

## 🎯 Key Takeaways

1. **Workspace** = Entire project (Grandine has 67 packages)
2. **Package** = Directory with Cargo.toml (like `types/`)  
3. **Crate** = What gets compiled (lib.rs or main.rs)
4. **Binary** = Has `main()`, can run (`grandine`)
5. **Library** = No `main()`, provides code (`types`, `bls`)
6. **Module** = Organization within crates (`phase0`, `altair`)

### Think of it like a building:
- **Workspace** = Entire building complex
- **Package** = Individual buildings
- **Crate** = Floors in a building
- **Module** = Rooms on a floor
- **Binary** = The main entrance (you can enter here)
- **Library** = Service areas (used by other parts)

## 💡 Practical Example: Adding New Code

To add a new validator feature to Grandine:

1. **Create a module** in existing package:
   ```rust
   // In types/src/lib.rs
   pub mod my_validator;
   ```

2. **Or create a new package**:
   ```bash
   mkdir my_validator
   cd my_validator
   cargo init --lib
   ```

3. **Add to workspace**:
   ```toml
   # Root Cargo.toml
   [workspace]
   members = [..., "my_validator"]
   ```

4. **Use in binary**:
   ```rust
   // grandine/src/main.rs
   use my_validator::validate;
   ```

This organization allows Grandine to:
- Compile efficiently (workspace shares compilation)
- Maintain clear boundaries (packages)
- Reuse code (libraries)
- Build multiple tools (multiple binaries)
- Scale to large codebases (67 packages!)