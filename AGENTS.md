# AGENTS.md: Developer & AI Agent Operations Manual

Welcome to **WML (Web Minimalistic Language)**. This repository houses the compiler, runtime substrate, and developer tooling for WML.

This guide provides AI agents and human contributors with the authoritative mental model, architectural rules, workspace layout, and development protocols required to contribute to this codebase safely and efficiently.

---

## 1. Project Mission & Core Tenets

WML is engineered to be the world's most cost-optimized, horizontal-scaling, full-stack web language, optimized for large language models and autonomous coding agents.

### North Star Tenets:
1. **Ultra-Low Memory Density (< 2.5MB Idle RAM):**
   - Zero Garbage Collector. No tracing overhead, no mark-sweep latency spikes.
   - **Request-Scoped Arena Allocation:** Memory allocated per HTTP request is bump-allocated in a 64KB slab and deallocated in $O(1)$ by resetting the pointer to zero.
2. **Hardware Target: x86_64 & ARM64 (aarch64):**
   - First-class support for both architectures from Day 1.
   - Optimized for physical-core density on AWS Graviton, Ampere Altra, and Hetzner ARM instances.
3. **Dual Execution Engines:**
   - **Cranelift JIT:** Sub-15ms dev compilation and instant hot reload.
   - **LLVM AOT:** Hyper-optimized, standalone static binary with LTO for release deployments.
4. **Modern PHP Colocation:**
   - Server-rendered HTML (`page`), backend endpoints (`route`), and client reactivity (`ui`) live in a single unified file (`.wml`).
   - Zero `node_modules`, zero npm, zero Webpack/Vite build steps.
   - Client interactivity compiles to vanilla JS targeting an embedded micro-signals engine (<2.5KB gzipped).
5. **Minimal AI Context Footprint (OWTD):**
   - *One Way To Do It*: Only `let`, only `fn`, only `for..in`, universal `??` coalescing.
   - Complete authentication flow in ~10 lines / 114 BPE tokens (`examples/auth.wml`).
6. **AI-First Diagnostic Protocol (`--format=ai-json`):**
   - Compilers must not emit ambiguous ANSI terminal squiggles to machines.
   - All errors emit machine-actionable JSON with byte-exact replacement patches (`AUTO_APPLY`) enabling autonomous 1-turn self-healing.

---

## 2. Workspace Crate Architecture

The WML compiler is structured as a modular Rust workspace under `crates/`:

```
crates/
├── wml-diagnostics/          # AI diagnostic protocol, JSON schemas, patch diffs
├── wml-syntax/               # Logos lexer, Pratt parser, la-arena AST handles, spans
├── wml-hir/                  # High-level IR, bidirectional type checker, scope analysis
├── wml-mir/                  # Block-argument SSA IR, async state machines, arena escape analysis
├── wml-runtime/              # Arena allocator, thread-per-core reactor, crypto, SIMD HTTP
├── wml-codegen-cranelift/    # Cranelift JIT engine for fast development loops
├── wml-codegen-llvm/         # LLVM AOT engine for production binaries (feature-gated)
├── wml-client/               # UI island extractor & micro-signals JS transpiler
└── wml-cli/                  # Unified developer CLI (`wml check`, `wml dev`, `wml build`)
```

### Dependency Invariants:
- `wml-diagnostics` has **zero compiler dependencies**; all crates can emit diagnostics.
- `wml-syntax` uses index-based arenas (`la-arena`) instead of deeply nested heap pointer graphs (`Box<AstNode>`) for cache locality and speed.
- `wml-codegen-llvm` must remain behind a Cargo feature flag (`--features llvm-backend`) so everyday development and CI tests do not incur heavy LLVM build times.
- `wml-runtime` must expose a pure C-ABI (`extern "C"`) so that both Cranelift and LLVM link against identical runtime symbols.

---

## 3. Strict Rules for AI Agents

When authoring, refactoring, or reviewing code in this repository:

1. **Never Introduce a Tracing Garbage Collector:**
   - Any dynamic memory on the request hot path must use the thread-local Request Arena.
   - Values surviving the request must be explicitly transferred to the persistent heap through `alloc.global`.
2. **Preserve Deterministic Parsing:**
   - All grammar rules must conform to the formal PEG grammar defined in `docs/spec/GRAMMAR.pest`.
   - Never introduce ambiguous grammar constructs (e.g., optional semicolons, dangling-else ambiguities, or overloaded punctuation).
3. **Always Emit AI-Parsable Diagnostics:**
   - Any compiler error or runtime panic must conform to `docs/spec/AI_DIAGNOSTIC_PROTOCOL.json`.
   - Always supply `byte_range` and, whenever possible, an atomic `suggested_patch` with calibrated confidence.
4. **Respect Thread-per-Core Boundaries:**
   - WML server workers use `monoio` / `io_uring` with `SO_REUSEPORT`.
   - Tasks are `!Send`. Never force cross-thread synchronization primitives (like `Arc<Mutex<T>>`) on request-scoped data.
5. **No External Frontend Dependencies:**
   - Do not add npm packages, Babel, Webpack, or Tailwind build chains.
   - Client reactivity must compile directly to the embedded micro-signals JS runtime.

---

## 4. Verification & Testing Protocol

Before completing any task, run the following verification steps:

```bash
# 1. Check workspace formatting & linting
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings

# 2. Run unit & integration tests
cargo test --workspace

# 3. Verify AI diagnostic emission on invalid samples
cargo run -p wml-cli -- check examples/auth.wml --format=ai-json
```

---

## 5. Master Specification References

- [Language Specification](file:///F:/DEV/WML/docs/spec/WML_LANGUAGE_SPECIFICATION.md)
- [Compiler & Runtime Architecture](file:///F:/DEV/WML/docs/wml_compiler_runtime_architecture.md)
- [Formal PEG Grammar](file:///F:/DEV/WML/docs/spec/GRAMMAR.pest)
- [AI Diagnostic Protocol](file:///F:/DEV/WML/docs/spec/AI_DIAGNOSTIC_PROTOCOL.json)
