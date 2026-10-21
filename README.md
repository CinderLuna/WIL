# WML (Web Minimalistic Language)

> **The ultra-low-memory, horizontal-scaling, full-stack web language engineered for autonomous coding agents and LLMs.**

[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-58%20passed-brightgreen.svg)]()
[![Rust](https://img.shields.io/badge/rust-2021%20edition-orange.svg)]()
[![Memory](https://img.shields.io/badge/idle%20RAM-%3C%202.5MB-blueviolet.svg)]()

---

## 1. Core Mission & Tenets

WML is designed from first principles to be the world's most cost-optimized, horizontal-scaling web language, eliminating accidental complexity for humans and autonomous coding agents alike.

### Key Pillars:
1. **Ultra-Low Memory Density (< 2.5MB Idle RAM):**
   - Zero Garbage Collector. No tracing overhead, no mark-sweep latency spikes.
   - **Request-Scoped Arena Allocation:** Memory allocated per HTTP request is bump-allocated in a 64KB slab and deallocated in $O(1)$ by resetting the pointer to zero.
2. **Dual Execution Engines:**
   - **Cranelift JIT:** Sub-15ms dev compilation and instant hot reload (`wml dev`).
   - **LLVM AOT:** Hyper-optimized, standalone static binary with LTO for release deployments (`wml build --release`).
3. **Hardware Target: x86_64 & ARM64 (aarch64):**
   - First-class support for both architectures with Day 1 parity.
   - Optimized for physical-core density on AWS Graviton, Ampere Altra, and Hetzner ARM instances.
4. **Modern PHP Colocation:**
   - Server-rendered HTML (`page`), backend endpoints (`route`), and client reactivity (`ui`) live in a single unified file (`.wml`).
   - Zero `node_modules`, zero npm, zero Webpack/Vite build steps.
   - Client interactivity compiles to vanilla JS targeting an embedded micro-signals engine (<2.5KB gzipped).
5. **Minimal AI Context Footprint (OWTD):**
   - *One Way To Do It*: Only `let`, only `fn`, only `for..in`, universal `??` coalescing.
   - Complete authentication flow in ~10 lines / 114 BPE tokens (`examples/auth.wml`).
6. **AI-First Diagnostic Protocol (`--format=ai-json` / `--ai`):**
   - All errors emit machine-actionable JSON with deterministic hash IDs, focus context slices, and atomic byte-exact replacement patches (`AUTO_APPLY`).
   - Automated self-healing engine (`wml fix --ai`) repairs syntax and type errors in a single turn.

---

## 2. Workspace Crate Architecture

The WML compiler is organized as a modular Rust workspace under `crates/`:

```
crates/
├── wml-diagnostics/          # AI diagnostic protocol, JSON schemas, patch diffs, atomic replacements
├── wml-syntax/               # Logos DFA lexer, Pratt parser, la-arena AST handles, source spans
├── wml-hir/                  # High-level IR, bidirectional typechecker, scope resolver, schema typing
├── wml-mir/                  # Block-argument SSA IR, async state machines, arena escape analysis
├── wml-runtime/              # Bump arena allocator, thread-per-core reactor, SIMD HTTP, native crypto
├── wml-codegen-cranelift/    # Cranelift JIT engine for fast development loops
├── wml-codegen-llvm/         # LLVM AOT engine for production binaries (x86_64 & aarch64)
├── wml-client/               # UI island extractor & micro-signals JS transpiler (<2.5KB)
└── wml-cli/                  # Developer CLI (`wml check`, `wml dev`, `wml build`, `wml fix`)
```

---

## 3. Quick Start & CLI Usage

### Building from Source
```bash
cargo build --package wml-cli --release
```

### 1. Validate & Typecheck (`wml check`)
```bash
# Human-readable diagnostics with context slices
wml check examples/auth.wml

# Machine-actionable AI diagnostic protocol
wml check examples/auth.wml --ai
```

### 2. Autonomous Self-Healing (`wml fix`)
```bash
# Inspect proposed unified diffs without modifying file
wml fix mutated_file.wml --dry-run --ai

# Atomically apply safe AUTO_APPLY patches (confidence >= 0.80)
wml fix mutated_file.wml --ai
```

### 3. Fast Development Server (`wml dev`)
```bash
# Starts JIT dev server with instant live reload
wml dev examples/counter.wml --port 3000
```

### 4. Production Release Build (`wml build`)
```bash
# Compile standalone release artifacts for current host
wml build examples/counter.wml --release

# Cross-compile for ARM64 (AWS Graviton, Ampere Altra)
wml build examples/counter.wml --release --target aarch64

# Cross-compile for x86_64 Linux
wml build examples/counter.wml --release --target x86_64
```

---

## 4. Canonical Language Samples

### Canonical Auth (`examples/auth.wml`)
```wml
// WML Canonical Full Auth Specification (~10 lines of executable logic)
// Zero imports. Zero ORM boilerplate. Argon2id + Ed25519/HMAC JWT native.

type User { id: int, email: str, hash: str }

route /auth/signup (post: {email: str, pass: str}) -> {token: str} {
    let hash = crypto.hash(pass)
    let user = db.insert(User, {email, hash})
    return {token: jwt.sign({sub: user.id})}
}

route /auth/login (post: {email: str, pass: str}) -> {token: str} {
    let user = db.get(User, email: email) ?? throw 401 "Invalid credentials"
    crypto.verify(pass, user.hash) ?? throw 401 "Invalid credentials"
    return {token: jwt.sign({sub: user.id})}
}

route /api/me (auth: true) -> User {
    return db.get(User, id: req.user.id) ?? throw 404
}
```

### Full-Stack Reactive Island (`examples/counter.wml`)
```wml
schema Stats {
    id: int pk auto,
    clicks: int default(0)
}

page /dashboard {
    let stat = db.get(Stats, id: 1) ?? db.insert(Stats, {clicks: 0})

    <main class="container">
        <h1>"Live Analytics Dashboard"</h1>
        <p>"Server clicks: " {stat.clicks}</p>
        
        <ClickIsland initial={stat.clicks} />
    </main>
}

ui ClickIsland(initial: int) {
    let count = signal(initial)

    action syncClicks(delta: int) -> int {
        let updated = db.update(Stats, id: 1, {clicks: clicks + delta})
        return updated.clicks
    }

    <div class="counter-card">
        <button onclick={() => count.value = count.value - 1}>"-"</button>
        <span class="count">{count.value}</span>
        <button onclick={() => {
            count.value = count.value + 1
            syncClicks(1)
        }}>+</button>
    </div>
}
```

### Real-Time Edge Feature Flag & Remote Config Control Plane (`examples/edgeflag.wml`)
```wml
// EdgeFlag: Real-Time Edge Feature Flag & Dynamic Remote Config Engine
// Single-file colocation: Schemas + SSR Dashboard + Reactive Micro-Signals Islands + High-Throughput Edge API
// Ultra-low memory density (<2.5MB RAM), zero GC latency, bump arena allocation, dual-target LLVM & Cranelift.

schema Flag {
    id: int pk auto,
    key: str,
    name: str,
    description: str,
    enabled: bool default(false),
    rollout_percentage: int default(0),
    environment: str default("production")
}

schema Metric {
    id: int pk auto,
    evaluations: int default(0),
    active_flags: int default(0)
}

// Full Walkthrough: docs/examples/EDGEFLAG_WALKTHROUGH.md
```

---

## 5. Specification Roadmap & Milestone Status

| Phase | Milestone | Deliverable | Status |
| :---: | :--- | :--- | :---: |
| **1** | PEG Grammar & AST Lexer/Parser | `wml-syntax` (logos DFA, Pratt parser, `la-arena` AST) | **Complete (PR #1)** |
| **2** | Type Inference & Semantic HIR | `wml-hir` (scope analyzer, schema typing, Hindley-Milner) | **Complete (PR #1)** |
| **3** | SSA MIR & Cranelift JIT | `wml-mir`, `wml-codegen-cranelift`, `wml-runtime` (bump arena) | **Complete (PR #2)** |
| **4** | Full-Stack SSR & Micro-Signals JS | `wml-client` (<2.5KB island signals), SSR streaming, bundler | **Complete (PR #3)** |
| **5** | LLVM AOT Release Pipeline | `wml-codegen-llvm` (x86_64 & ARM64 dual-target, Clang LTO) | **Complete (PR #4)** |
| **6** | AI Diagnostic & Self-Healing Engine | `wml-diagnostics`, `wml check --ai`, `wml fix --ai` | **Complete (PR #5)** |

---

## 6. Architecture & Specification References

- [Language Specification](docs/spec/WML_LANGUAGE_SPECIFICATION.md)
- [Compiler & Systems Architecture](docs/wml_compiler_runtime_architecture.md)
- [Formal PEG Grammar](docs/spec/GRAMMAR.pest)
- [AI Diagnostic Protocol Schema](docs/spec/AI_DIAGNOSTIC_PROTOCOL.json)
- [Phase 3 JIT & MIR Architecture Guide](docs/phase3_architecture_guide.md)
- [Phase 4 Full-Stack & Island Hydration Guide](docs/phase4_fullstack_and_bundler_guide.md)
- [Phase 5 LLVM AOT Release Pipeline Guide](docs/phase5_llvm_aot_guide.md)
- [Phase 6 AI Diagnostic & Self-Healing Guide](docs/phase6_ai_diagnostics_guide.md)
- [EdgeFlag Full-Stack Walkthrough & Architecture](docs/examples/EDGEFLAG_WALKTHROUGH.md)
- [Developer & AI Operations Manual](AGENTS.md)

---

## 7. License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
