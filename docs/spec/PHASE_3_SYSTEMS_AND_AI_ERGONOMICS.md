# WML Phase 3: Systems Architecture & AI Ergonomics Recommendations
## Runtime Diagnostics, CLI Dev Workflow, and Handler Ergonomics

**Author:** Lead Language Designer & AI Ergonomics Architect  
**Target:** Implementation of Phase 3 (`wml-mir`, `wml-runtime`, `wml-codegen-cranelift`, `wml-cli`)  
**Status:** Approved Architectural Blueprint  

---

## 1. Runtime & MIR Error Diagnostics (`AI_DIAGNOSTIC_PROTOCOL.json`)

To enable autonomous self-healing, errors occurring during MIR lowering, escape analysis, and Cranelift JIT runtime execution must conform to the existing diagnostic protocol schema (`docs/spec/AI_DIAGNOSTIC_PROTOCOL.json`).

### 1.1 Error Taxonomy & Standardized Error Codes

| Phase / Subsystem | Diagnostic Code | Category | Description | Machine-Actionable Patch Strategy |
| :--- | :--- | :--- | :--- | :--- |
| **MIR Escape Analysis** | `WML-MEM-4001` | `SecurityViolation` / `MemorySafety` | Request-scoped arena reference escapes into persistent/global scope | Wrap allocation in `alloc.global(...)` or deep clone |
| **MIR Async Lowering** | `WML-MIR-4002` | `TypeMismatch` | Unyielded coroutine or generator without yield points | Suggest `return` or insert explicit yield |
| **Cranelift ABI Call** | `WML-ABI-4003` | `TypeMismatch` | Native C-ABI type mismatch between JIT trampoline and runtime | Auto-cast primitive integer/pointer width |
| **Runtime Panic (JIT)** | `WML-RUN-5001` | `RuntimePanic` | Uncaught runtime trap (divide by zero, index out of bounds) | Suggest guard condition (`if denom == 0` or `?? throw 400`) |
| **HTTP SIMD Parsing** | `WML-NET-6001` | `RuntimePanic` | Malformed HTTP request stream, header overflow (>8KB) | Suggest client payload adjustment or header size limit |
| **Auth/JWT Verification** | `WML-SEC-5002` | `SecurityViolation` | Missing or expired JWT token on `auth: true` route | Auto-generate auth header check / bearer extraction |

### 1.2 Concrete Diagnostic Payloads

#### Example 1: Request Arena Escape During MIR Lowering (`WML-MEM-4001`)

```json
{
  "protocol_version": "1.0.0",
  "status": "error",
  "diagnostics": [
    {
      "id": "diag_mir_esc_001",
      "code": "WML-MEM-4001",
      "category": "SecurityViolation",
      "severity": "error",
      "message": "Escape analysis failure: Reference to request-scoped arena buffer assigned to persistent global cache `GLOBAL_USERS`.",
      "primary_location": {
        "file": "app.wml",
        "span": { "start_line": 14, "start_col": 5, "end_line": 14, "end_col": 38 },
        "byte_range": { "start": 320, "end": 353 }
      },
      "context_slice": {
        "focus_line": 14,
        "lines": [
          { "line_number": 13, "content": "route /cache (post: {key: str, val: str}) {", "is_focus": false },
          { "line_number": 14, "content": "    GLOBAL_USERS.insert(key, val)", "is_focus": true },
          { "line_number": 15, "content": "    return {ok: true}", "is_focus": false }
        ]
      },
      "suggested_patch": {
        "patch_type": "AUTO_APPLY",
        "confidence": 0.95,
        "explanation": "Promote request arena allocation to persistent global heap via `alloc.global(val)` before caching.",
        "replacement": {
          "file": "app.wml",
          "byte_range": { "start": 344, "end": 347 },
          "new_text": "alloc.global(val)"
        },
        "patch_unified": "--- app.wml\n+++ app.wml\n@@ -14,1 +14,1 @@\n-    GLOBAL_USERS.insert(key, val)\n+    GLOBAL_USERS.insert(key, alloc.global(val))"
      }
    }
  ]
}
```

#### Example 2: Runtime JIT Division by Zero Trap (`WML-RUN-5001`)

```json
{
  "protocol_version": "1.0.0",
  "status": "error",
  "diagnostics": [
    {
      "id": "diag_run_trap_002",
      "code": "WML-RUN-5001",
      "category": "RuntimePanic",
      "severity": "error",
      "message": "Runtime trap in Cranelift JIT handler: Integer division by zero.",
      "primary_location": {
        "file": "calc.wml",
        "span": { "start_line": 8, "start_col": 18, "end_line": 8, "end_col": 26 },
        "byte_range": { "start": 164, "end": 172 }
      },
      "suggested_patch": {
        "patch_type": "USER_CONFIRM",
        "confidence": 0.88,
        "explanation": "Guard against zero divisor using null-coalescing throw expression.",
        "replacement": {
          "file": "calc.wml",
          "byte_range": { "start": 164, "end": 172 },
          "new_text": "b == 0 ? throw 400 \"Divisor cannot be zero\" : a / b"
        }
      }
    }
  ]
}
```

---

## 2. Developer CLI Workflow: `wml dev` & `wml run`

### 2.1 The Two Execution Modes

WML provides two primary developer execution commands:
1. `wml dev <file.wml>`:
   - Starts the thread-per-core Cranelift JIT HTTP development server.
   - Watches `<file.wml>` and all included modules for changes.
   - On change: Triggers sub-15ms incremental compilation, atomically swapping the handler dispatch table via `AtomicPtr`.
   - Never crashes on compilation errors: keeps old handlers running and emits structured `AI_DIAGNOSTIC_PROTOCOL` JSON to stderr.
2. `wml run <file.wml>`:
   - One-shot execution: compiles and executes `<file.wml>` in-memory via Cranelift JIT, or starts server until terminated without file watching.
   - Optimized for CI pipelines, automated integration tests, and autonomous AI agents testing API endpoints.

### 2.2 Machine-Readable CLI Lifecycle Events (`--format=ai-json`)

When `--format=ai-json` is passed to `wml dev` or `wml run`, standard console text is replaced by a single-line streaming JSONL protocol on stdout/stderr. This allows autonomous coding agents and IDEs to monitor server readiness, port binding, and compilation metrics without brittle regex parsing.

#### Event 1: Server Startup (`server_ready`)
```json
{"event":"server_ready","port":3000,"host":"127.0.0.1","routes":[{"method":"POST","path":"/auth/signup"},{"method":"POST","path":"/auth/login"},{"method":"GET","path":"/api/me"}],"compile_duration_ms":11.8,"timestamp":"2026-10-21T19:30:00Z"}
```

#### Event 2: Hot Code Reloading Succeeded (`hmr_reloaded`)
```json
{"event":"hmr_reloaded","file":"auth.wml","compile_duration_ms":4.2,"timestamp":"2026-10-21T19:30:05Z"}
```

#### Event 3: Hot Code Reloading Failed with Diagnostic (`hmr_error`)
```json
{"event":"hmr_error","file":"auth.wml","report":{"protocol_version":"1.0.0","status":"error","diagnostics":[...]},"timestamp":"2026-10-21T19:30:10Z"}
```

---

## 3. OWTD Simplicity & Minimal Token Overhead for Handlers

### 3.1 The 4 Principles of OWTD Handler Ergonomics

1. **Declarative Signature Parameter Injection**:
   - Routes declare their exact input body and query parameters directly in the route signature.
   - SIMD JSON/URL-form parser parses directly into request arena slots; variables are injected into scope.
   - **Zero boilerplate**: No `req.body`, no `let body = req.json()`, no manual type casting.

2. **Implicit Response Serialization**:
   - The final expression or `return` value is automatically serialized as JSON with `Content-Type: application/json` and `Status: 200 OK`.
   - String returns default to `Content-Type: text/html; charset=utf-8`.
   - Binary buffer returns stream raw bytes with `Content-Type: application/octet-stream`.

3. **Universal `throw <code>` Exit**:
   - Errors are not returned as complex `Result<Response, AppError>` types that AI agents struggle to format.
   - Any HTTP error is triggered with `throw <status_code> [message]`.
   - Universal null/error coalescing with `??`:
     ```wml
     let user = db.get(User, id: req.user.id) ?? throw 404 "User not found"
     ```

4. **Ambient Web Primitives (Zero Imports)**:
   - `req`: Ambient request context (`req.headers`, `req.ip`, `req.user`).
   - `res`: Ambient response modifier (`res.set_cookie(...)`, `res.set_header(...)`).
   - `db`: Direct type-checked SQL queries on request arena.
   - `crypto`: Argon2id password hashing and constant-time comparison.
   - `jwt`: RFC 7519 token generation and decoding.

### 3.2 Token Efficiency Comparison

| Framework / Stack | Lines of Code | Estimated Tokens (cl100k) | Complexity / Pitfalls |
| :--- | :--- | :--- | :--- |
| **Next.js + Prisma + TypeScript** | 68 lines | ~680 tokens | Imports, async/await, try/catch, Response wrapper, zod validation |
| **Go + Gin + GORM** | 54 lines | ~520 tokens | Struct tags, pointer receivers, err != nil checks, context passing |
| **Rust + Axum + SQLx** | 72 lines | ~750 tokens | State extractors, async closures, Json(payload), Result mapping |
| **WML (Modern PHP Model)** | **12 lines** | **114 tokens** | **Zero imports, native validation, universal ?? throw** |

---

## 4. Phase 3 Concrete Architecture Checklist

### Crate: `wml-mir`
- [ ] Implement HIR -> MIR lowering for `route`, `let`, `return`, `throw`, and `??`.
- [ ] Implement Request Arena Escape Analysis (`WML-MEM-4001`).
- [ ] Implement Basic Block CFG with block arguments (no phi nodes).

### Crate: `wml-runtime`
- [ ] SIMD HTTP parsing using `httparse` allocating directly into the thread-local `RequestArena`.
- [ ] C-ABI exports: `wml_arena_alloc(size, align)`, `wml_arena_reset()`, `wml_crypto_hash(...)`, `wml_jwt_sign(...)`.
- [ ] Thread-per-core HTTP server reactor (monoio / tokio thread pool).

### Crate: `wml-codegen-cranelift`
- [ ] Lower MIR basic blocks to Cranelift IR (`clif`).
- [ ] JIT module compilation emitting function pointer trampolines.
- [ ] Atomic function pointer swapping for sub-15ms HMR.

### Crate: `wml-cli`
- [ ] Implement `wml dev <file.wml>` with file watcher and atomic hot reloading.
- [ ] Implement `wml run <file.wml>` for one-shot execution.
- [ ] Implement `--format=ai-json` event stream (`server_ready`, `hmr_reloaded`, `hmr_error`).
