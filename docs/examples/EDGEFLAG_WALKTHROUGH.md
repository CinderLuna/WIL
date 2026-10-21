# EdgeFlag: Real-Time Edge Feature Flag & Dynamic Remote Config Engine

> **A production-grade, single-file full-stack web application built in WML (Web Minimalistic Language).**

---

## 1. Executive Summary

**EdgeFlag** is a high-throughput, low-latency feature flagging and dynamic remote configuration control plane designed to run in ultra-low-memory environments (<2.5MB RAM) such as edge points of presence (PoPs), AWS Graviton ARM64 instances, and containerized micro-vms.

It exemplifies the core design tenets of WML:
1. **Single-File Modern PHP Colocation (`examples/edgeflag.wml`):** Schemas, server-rendered dashboard, reactive client islands, auto-wired server actions, and high-throughput edge API routes all live in a single `.wml` file.
2. **Zero Tracing Garbage Collector:** Evaluates feature flags in microseconds using the 64KB request-scoped bump arena (`RequestArena`) with $O(1)$ memory reset.
3. **Embedded Micro-Signals Client Reactivity:** Client-side toggle switches and dynamic rollout sliders compile to vanilla JS targeting WML's embedded reactive signals runtime (<2.5KB gzipped, zero npm, zero `node_modules`).
4. **Dual-Target Execution:** Instant sub-15ms dev compilation via Cranelift JIT (`wml dev`) and hyper-optimized standalone native binaries for x86_64 and ARM64 via LLVM AOT (`wml build --release`).
5. **Native Security Primitives:** API key provisioning with native Argon2id password hashing and Ed25519/HMAC JWT signing with zero external library overhead.

---

## 2. Architecture & Request Flow

```
+--------------------------------------------------------------------------------------------------+
|                                    examples/edgeflag.wml                                         |
|                                                                                                  |
|  [ Data Layer: Schemas ]                                                                         |
|  - schema Flag (id, key, name, description, enabled, rollout_percentage, environment)            |
|  - schema Metric (id, evaluations, active_flags)                                                 |
|  - schema EdgeKey (id, name, secret_hash, environment)                                           |
|                                                                                                  |
|  [ Server SSR Page: page / ]                                                                     |
|  - Direct zero-overhead database access via `db.get` and bump-allocated HTML streaming           |
|  - Hydrates `<FlagControlIsland initial_flags={...} initial_evals={...} />`                      |
|                                                                                                  |
|  [ Client Reactive Island: ui FlagControlIsland ]                                                |
|  - Compiles to fine-grained signals (`active`, `evals`, `percentage`)                            |
|  - Direct DOM mutation: zero Virtual DOM diffing                                                 |
|  - Auto-wires RPC server actions (`toggleFlag`, `updateRollout`, `simulateEval`)                 |
|                                                                                                  |
|  [ Edge API Routes: route /api/v1/... ]                                                          |
|  - POST /api/v1/auth/provision: Native Argon2id + JWT token issuance                             |
|  - POST /api/v1/evaluate: Microsecond bump-allocated flag evaluation                             |
|  - GET  /api/v1/flags/sync: Authenticated edge state synchronization                             |
+--------------------------------------------------------------------------------------------------+
                                        |
                 +----------------------+----------------------+
                 v                                             v
+----------------------------------+       +-----------------------------------+
|         Server Runtime           |       |        Client Reactive JS         |
|  - Cranelift JIT (Dev loop)      |       |  - Embedded Micro-Signals Core    |
|  - LLVM AOT (Release static bin) |       |  - < 2.5KB gzipped, 0 npm deps    |
|  - Bump Arena (64KB slab, O(1))  |       |  - Auto-mounts on DOMContentLoaded|
+----------------------------------+       +-----------------------------------+
```

---

## 3. Code Deep Dive: `examples/edgeflag.wml`

### 3.1 Declarative Schemas
No ORM mappings or migration configuration needed. Schemas map directly to typed database structures:
```wml
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

schema EdgeKey {
    id: int pk auto,
    name: str,
    secret_hash: str,
    environment: str default("production")
}
```

### 3.2 Server-Rendered Page (`page /`)
Server pages execute on the native backend, streaming HTML directly to the HTTP socket while injecting reactive islands:
```wml
page / {
    let metric = db.get(Metric, id: 1) ?? db.insert(Metric, {evaluations: 1042, active_flags: 3})
    let flag1 = db.get(Flag, id: 1) ?? db.insert(Flag, {
        key: "dark_mode_v2",
        name: "Dark Mode Theme",
        description: "Enables new OLED high-contrast dark theme",
        enabled: true,
        rollout_percentage: 100,
        environment: "production"
    })

    <main class="dashboard">
        <header class="header">
            <div class="logo-box">
                <h1 class="title">"EdgeFlag Engine"</h1>
                <span class="badge">"ARM64/x86_64 Edge Node"</span>
            </div>
            <p class="subtitle">"Sub-2.5MB RAM Zero-GC Feature Flag & Remote Config Control Plane"</p>
        </header>

        <section class="metrics-grid">
            <div class="metric-card">
                <span class="metric-label">"Active Flags: "</span>
                <span class="metric-val">{metric.active_flags}</span>
            </div>
            <div class="metric-card">
                <span class="metric-label">"Edge Evaluations: "</span>
                <span class="metric-val">{metric.evaluations}</span>
            </div>
            <div class="metric-card">
                <span class="metric-label">"Arena Slab: "</span>
                <span class="metric-val">"64 KB Bump Allocated"</span>
            </div>
        </section>

        // Hydrated Client Island
        <FlagControlIsland initial_flags={metric.active_flags} initial_evals={metric.evaluations} />
    </main>
}
```

### 3.3 Reactive Client Island (`ui FlagControlIsland`)
Client islands compile directly to vanilla JavaScript targeting the embedded micro-signals core. Typed RPC actions defined inside the island are auto-wired by the WML compiler into POST endpoints:
```wml
ui FlagControlIsland(initial_flags: int, initial_evals: int) {
    let active = signal(initial_flags)
    let evals = signal(initial_evals)
    let percentage = signal(50)

    action toggleFlag(flag_id: int, state: bool) -> int {
        let updated = db.update(Metric, id: 1, {active_flags: active_flags + 1})
        return updated.active_flags
    }

    action updateRollout(flag_id: int, pct: int) -> int {
        let updated = db.update(Flag, id: flag_id, {rollout_percentage: pct})
        return updated.rollout_percentage
    }

    action simulateEval(key: str) -> int {
        let updated = db.update(Metric, id: 1, {evaluations: evaluations + 1})
        return updated.evaluations
    }

    <div class="flag-control-panel">
        <div class="card">
            <h3>"Feature: AI Search Engine (ai_search_v3)"</h3>
            <p>"Status: Active on Edge Reactor"</p>
            <div class="btn-group">
                <button onclick={
                    active += 1;
                    evals += 10;
                    toggleFlag(1, true);
                }>
                    "Toggle Flag (Active: " {active} ")"
                </button>
                <button onclick={
                    percentage += 10;
                    updateRollout(1, percentage);
                }>
                    "Bump Rollout (+10%): " {percentage} "%"
                </button>
                <button onclick={
                    evals += 1;
                    simulateEval("ai_search_v3");
                }>
                    "Simulate Evaluation (Pings: " {evals} ")"
                </button>
            </div>
        </div>
    </div>
}
```

### 3.4 Microsecond Edge API Routes
```wml
// High-Throughput Edge API: Provision API Keys with native Argon2id + HMAC/Ed25519 JWT
route /api/v1/auth/provision (post: {name: str, secret: str}) -> {token: str} {
    let secret_hash = crypto.hash(secret)
    let key = db.insert(EdgeKey, {name, secret_hash})
    return {token: jwt.sign({sub: key.id})}
}

// Edge Flag Evaluation Endpoint: Microsecond bump-allocated request path
route /api/v1/evaluate (post: {key: str, entity_id: str}) -> {key: str, enabled: bool, variant: str} {
    let flag = db.get(Flag, key: key) ?? throw 404 "Flag not found"
    return {key: flag.key, enabled: flag.enabled, variant: "control"}
}

// Edge Sync Status Endpoint
route /api/v1/flags/sync (auth: true) -> Metric {
    return db.get(Metric, id: 1) ?? throw 404
}
```

---

## 4. Running & Verifying EdgeFlag

### 4.1 Typecheck with AI Diagnostic Protocol
```bash
cargo run -p wml-cli -- check examples/edgeflag.wml --ai
```
**Output:**
```json
{
  "protocol_version": "1.0.0",
  "status": "ok",
  "summary": {
    "total_errors": 0,
    "total_warnings": 0,
    "auto_fixable": 0
  },
  "compilation_metrics": {
    "duration_ms": 3.65,
    "lines_parsed": 138,
    "tokens_evaluated": 796,
    "target": "check-only"
  },
  "diagnostics": []
}
```

### 4.2 Run JIT Development Server
```bash
cargo run -p wml-cli -- dev examples/edgeflag.wml --port 3000
```
**Output:**
```text
⚡ [WML JIT] Compiled 11 routes in 120ms
🚀 [WML Server] Listening on http://127.0.0.1:3000
```

### 4.3 Compile Standalone Production Release (Dual Target)
```bash
# Build for host
cargo run -p wml-cli -- build examples/edgeflag.wml --release --out-dir dist/edgeflag

# Cross-compile for ARM64 (AWS Graviton, Ampere Altra)
cargo run -p wml-cli -- build examples/edgeflag.wml --release --target aarch64 --out-dir dist/edgeflag-arm64

# Cross-compile for x86_64
cargo run -p wml-cli -- build examples/edgeflag.wml --release --target x86_64 --out-dir dist/edgeflag-x86_64
```
**Generated Artifacts:**
- `dist/edgeflag/manifest.json`: Full route, island, and asset manifest.
- `dist/edgeflag/assets/islands.js`: Transpiled micro-signals reactive runtime and island component.
- `dist/edgeflag/app.ll`: Fully optimized LLVM IR.
- `dist/edgeflag/bin/app.obj` / `app.o`: Native compiled object file ready for standalone static linking.
