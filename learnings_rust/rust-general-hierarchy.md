# Rust Organization Hierarchy - General Concepts

## 📊 Complete Hierarchy Diagram

```
┌──────────────────────────────────────────────────────────────┐
│                         WORKSPACE                             │
│                    (Optional - for multi-package projects)    │
│                         [workspace]                           │
│                         Cargo.toml                            │
│                                                               │
│  ┌─────────────────────────────────────────────────────┐     │
│  │                      PACKAGE                         │     │
│  │                  (Has Cargo.toml)                    │     │
│  │                                                      │     │
│  │   ┌────────────────────────────────────────────┐    │     │
│  │   │                 CRATES                      │    │     │
│  │   │         (Compilation units in package)      │    │     │
│  │   │                                             │    │     │
│  │   │  ┌──────────────────┐  ┌─────────────────┐ │    │     │
│  │   │  │   BINARY CRATE    │  │  LIBRARY CRATE  │ │    │     │
│  │   │  │   (src/main.rs)   │  │  (src/lib.rs)   │ │    │     │
│  │   │  │   Has main()      │  │  No main()      │ │    │     │
│  │   │  │   Executable      │  │  Reusable code  │ │    │     │
│  │   │  └──────────────────┘  └─────────────────┘ │    │     │
│  │   │                                             │    │     │
│  │   │         ┌──────────────────┐               │    │     │
│  │   │         │     MODULES      │               │    │     │
│  │   │         │  (mod keyword)   │               │    │     │
│  │   │         │                  │               │    │     │
│  │   │         │  ┌──────────┐   │               │    │     │
│  │   │         │  │SUBMODULES│   │               │    │     │
│  │   │         │  │          │   │               │    │     │
│  │   │         │  └──────────┘   │               │    │     │
│  │   │         └──────────────────┘               │    │     │
│  │   └────────────────────────────────────────────┘    │     │
│  └─────────────────────────────────────────────────────┘     │
└──────────────────────────────────────────────────────────────┘
```

## 🎯 Simple Top-to-Bottom Hierarchy

```
                    WORKSPACE (optional)
                         │
                    ┌────┴────┐
                    │         │
                PACKAGE   PACKAGE
                    │
            ┌───────┼───────┐
            │               │
      BINARY CRATE    LIBRARY CRATE
      (executable)    (reusable code)
            │               │
        MODULES         MODULES
            │               │
       SUBMODULES      SUBMODULES
```

## 📦 Conceptual Hierarchy

```
WORKSPACE
  └── Multiple related packages
      └── PACKAGE
          └── One or more crates + Cargo.toml
              └── CRATE
                  ├── BINARY (has main(), executable)
                  └── LIBRARY (no main(), importable)
                      └── MODULE
                          └── Code organization unit
                              └── SUBMODULE
                                  └── Nested organization
```

## 🔑 Key Relationships

```
┌─────────────────────────────────────────────────┐
│ LARGEST SCOPE                                   │
│                                                 │
│  WORKSPACE                                      │
│  • Collection of packages                       │
│  • Shares Cargo.lock                           │
│  • Optional (single package projects don't need)│
│                                                 │
│    ↓ contains                                   │
│                                                 │
│  PACKAGE                                        │
│  • Has Cargo.toml                              │
│  • Build unit                                  │
│  • Can have multiple crates                    │
│                                                 │
│    ↓ contains                                   │
│                                                 │
│  CRATE                                         │
│  • Compilation unit                            │
│  • Tree of modules                             │
│  • Either binary or library                    │
│                                                 │
│    ↓ types                                     │
│                                                 │
│  ┌─────────────┐      ┌──────────────┐        │
│  │   BINARY    │      │   LIBRARY    │        │
│  │ • main()    │      │ • No main()  │        │
│  │ • Runnable  │      │ • Importable │        │
│  └─────────────┘      └──────────────┘        │
│                                                 │
│    ↓ contains                                   │
│                                                 │
│  MODULE                                         │
│  • Namespace                                    │
│  • Privacy boundary                             │
│  • Can be file or inline                        │
│                                                 │
│ SMALLEST SCOPE                                  │
└─────────────────────────────────────────────────┘
```

## 🎨 Visual Analogy

```
Think of it as a Library System:

📚 WORKSPACE = Library Network
   │
   ├── 📖 PACKAGE = Individual Library Branch
   │     │
   │     ├── 🎯 BINARY CRATE = Information Desk (main entrance)
   │     │     • Has main() - where you start
   │     │     • Can be run/executed
   │     │
   │     └── 📚 LIBRARY CRATE = Book Collection
   │           • No main() - just resources
   │           • Used by other code
   │           │
   │           ├── 📁 MODULE = Book Section (Fiction, Science)
   │           │     │
   │           │     └── 📄 SUBMODULE = Shelf (Sci-Fi, Mystery)
   │           │
   │           └── 📁 MODULE = Another Section
   │
   └── 📖 PACKAGE = Another Library Branch
```

## 🔄 Compilation & Execution Flow

```
         WORKSPACE
              │
     ┌────────┴────────┐
     │                 │
  PACKAGE A        PACKAGE B
     │                 │
     ├── lib.rs        ├── main.rs (executable)
     │   └── MODULE    │   └── uses Package A
     │                 │
     └── Cargo.toml    └── Cargo.toml
                           dependencies:
                           package_a = "..."
```

## 📊 Comparison Table

| Concept | Purpose | Has Cargo.toml | Can Execute | Can Import |
|---------|---------|----------------|-------------|------------|
| **Workspace** | Manage multiple packages | ✅ (root) | ❌ | ❌ |
| **Package** | Bundle crates + dependencies | ✅ | Depends | Depends |
| **Binary Crate** | Executable program | ❌ | ✅ | ❌ |
| **Library Crate** | Reusable code | ❌ | ❌ | ✅ |
| **Module** | Code organization | ❌ | ❌ | ✅ (within crate) |

## 🏗️ Structure Examples

### Single Package Project
```
my_app/
├── Cargo.toml         (PACKAGE)
└── src/
    └── main.rs        (BINARY CRATE)
        └── mod utils  (MODULE)
```

### Library Package
```
my_lib/
├── Cargo.toml         (PACKAGE)
└── src/
    └── lib.rs         (LIBRARY CRATE)
        ├── mod math   (MODULE)
        └── mod io     (MODULE)
```

### Package with Both Binary and Library
```
my_tool/
├── Cargo.toml         (PACKAGE)
└── src/
    ├── main.rs        (BINARY CRATE)
    └── lib.rs         (LIBRARY CRATE)
```

### Workspace with Multiple Packages
```
my_workspace/
├── Cargo.toml         (WORKSPACE)
│   [workspace]
│   members = ["app", "lib"]
├── app/
│   ├── Cargo.toml     (PACKAGE)
│   └── src/
│       └── main.rs    (BINARY CRATE)
└── lib/
    ├── Cargo.toml     (PACKAGE)
    └── src/
        └── lib.rs     (LIBRARY CRATE)
```

## 🔗 Dependency Direction

```
                Higher Level
                     ↑
    WORKSPACE ← manages multiple
        ↑
    PACKAGE ← defines dependencies
        ↑
    CRATE ← compilation boundary
        ↑
    MODULE ← organization within crate
        ↑
    CODE ← actual implementation
                Lower Level
```

## 💡 Key Points

1. **WORKSPACE**: Optional, for multi-package projects
2. **PACKAGE**: Has Cargo.toml, contains crate(s)
3. **CRATE**: What Rust compiler actually compiles
4. **BINARY**: Executable with main() function
5. **LIBRARY**: Reusable code without main()
6. **MODULE**: Organization within a crate

The hierarchy is:
- **Workspace** (optional) contains →
- **Packages** which contain →
- **Crates** (binary or library) which contain →
- **Modules** which can contain →
- **Submodules** and actual code