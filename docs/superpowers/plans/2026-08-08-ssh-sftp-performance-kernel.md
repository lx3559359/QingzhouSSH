# SSH Session and SFTP Performance Kernel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reuse trusted SSH sessions, eliminate stale directory races and duplicate network hashing, add bounded pipelined transfers, and expose stable speed, ETA, verification, and phase metrics without weakening the existing SSH/SFTP safety boundary.

**Architecture:** `ServerConnector` gains a shared per-server connection pool whose entries hold an `Arc`-backed authenticated transport and cached capabilities. SFTP keeps independent channels per operation, chooses remote hash commands only from probed fixed capabilities, and uses a bounded offset-read pipeline for downloads while the existing `russh-sftp` write pipeline handles uploads. A deterministic progress tracker throttles IPC and supplies EWMA speed and ETA to the existing React transfer view.

**Tech Stack:** Rust 2021, Tokio, russh 0.62.4, russh-sftp 2.4.0, Tauri 2, React 19, TypeScript, Vitest, Testing Library, project-local AsyncSSH fixture.

**Design reference:** `docs/superpowers/specs/2026-08-08-cross-platform-task-sftp-optimization-design.md`

---

## File responsibilities

- Modify `src-tauri/src/core/ssh/transport.rs` to make authenticated transports shareable and observable without exposing credentials.
- Modify `src-tauri/src/services/server_connector.rs` to own the shared pool, capability TTL, per-server connection lock, invalidation, and testable reuse policy.
- Modify `src-tauri/src/services/app_services.rs` and SSH-consuming services to reuse the shared connector and stop eagerly disconnecting healthy pooled transports.
- Modify `src/features/file-browser/directorySessionCache.ts` and its tests to add a five-second fresh window and generation-safe refresh semantics.
- Modify `src/features/transfers/FileTransferPage.tsx` and tests so obsolete directory responses cannot replace the active path.
- Modify `src-tauri/src/core/sftp/transfer.rs` to add verification policies, remote fixed-command hashes, tuned SFTP configuration, progress tracking, and bounded offset-read downloads.
- Modify `src-tauri/src/services/transfer_service.rs`, `src-tauri/src/domain/events.rs`, frontend API contracts, preview API, and transfer UI to carry verification and timing data.
- Modify the project SSH fixture and live tests to count transports and remote hash invocations.
- Create `scripts/benchmark-sftp.ps1` and `scripts/tests/sftp-performance.tests.ps1` for repeatable JSON benchmark evidence under `artifacts/benchmarks`.
- Update public support/security/user documentation only after behavior is verified.

## Task 1: Shareable authenticated SSH transport

**Files:**

- Modify: `src-tauri/src/core/ssh/transport.rs`

- [ ] **Step 1: Write failing unit tests**

Add tests proving a session can be cloned, both clones report the same closed state, and `disconnect` no longer consumes the wrapper. The compile-time shape is:

```rust
fn assert_clone<T: Clone>() {}

#[test]
fn authenticated_session_is_shareable() {
    assert_clone::<AuthenticatedSshSession>();
}
```

Keep network behavior in the live test added in Task 3; do not create a fake `russh::client::Handle`.

- [ ] **Step 2: Run the focused test to verify RED**

Run:

```powershell
. .\scripts\dev-env.ps1 -Quiet
$env:CARGO_BUILD_JOBS='1'
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --lib core::ssh::transport::tests::authenticated_session_is_shareable -- --exact
```

Expected: compilation fails because `AuthenticatedSshSession` does not implement `Clone`.

- [ ] **Step 3: Introduce an Arc-backed inner session**

Implement this ownership shape:

```rust
#[derive(Clone)]
pub struct AuthenticatedSshSession {
    inner: Arc<AuthenticatedSshSessionInner>,
}

struct AuthenticatedSshSessionInner {
    handle: client::Handle<HostKeyVerifier>,
    timeout: Duration,
}

impl AuthenticatedSshSession {
    pub async fn open_session_channel(&self) -> AppResult<russh::Channel<client::Msg>> {
        Ok(self.inner.handle.channel_open_session().await?)
    }

    pub fn is_closed(&self) -> bool {
        self.inner.handle.is_closed()
    }

    pub async fn disconnect(&self) {
        let _ = self
            .inner
            .handle
            .disconnect(Disconnect::ByApplication, "", "")
            .await;
    }

    pub fn timeout(&self) -> Duration {
        self.inner.timeout
    }
}
```

Construct the `Arc` only after authentication succeeds. Update internal field access in `execute_authenticated` and related helpers to use `session.inner.handle`.

- [ ] **Step 4: Run transport tests to verify GREEN**

Run:

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --lib core::ssh::transport::tests
```

Expected: all transport tests pass.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/core/ssh/transport.rs
git commit -m "refactor: make authenticated SSH sessions shareable"
```

## Task 2: Shared per-server session and capability pool

**Files:**

- Modify: `src-tauri/src/services/server_connector.rs`
- Modify: `src-tauri/src/services/mod.rs`

- [ ] **Step 1: Add failing policy tests**

Extract pure policy inputs so reuse can be tested without network handles:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
struct ConnectionIdentity {
    host: String,
    port: u16,
    username: String,
    credential_id: String,
    fingerprint_sha256: String,
}

#[test]
fn reuses_only_matching_open_idle_entries() {
    let now = Instant::now();
    assert!(may_reuse(true, true, now, now, now));
    assert!(!may_reuse(false, true, now, now, now));
    assert!(!may_reuse(true, false, now, now, now));
    assert!(!may_reuse(
        true,
        true,
        now - SESSION_IDLE_TTL - Duration::from_millis(1),
        now,
        now,
    ));
}

#[test]
fn capabilities_expire_independently_from_transport() {
    let now = Instant::now();
    assert!(capabilities_are_fresh(now - Duration::from_secs(1), now));
    assert!(!capabilities_are_fresh(
        now - CAPABILITY_TTL - Duration::from_millis(1),
        now,
    ));
}
```

Use `SESSION_IDLE_TTL = 90 seconds` and `CAPABILITY_TTL = 10 minutes`.

- [ ] **Step 2: Verify RED**

Run:

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --lib services::server_connector::tests
```

Expected: compilation fails because pool policy types and functions do not exist.

- [ ] **Step 3: Implement pool state and per-server locks**

Make `ConnectedServer` cloneable and add these internal records:

```rust
#[derive(Clone)]
pub struct ConnectedServer {
    pub profile: ServerProfile,
    pub session: AuthenticatedSshSession,
    pub capabilities: SystemCapabilities,
    pub redactor: Redactor,
}

struct CachedConnection {
    identity: ConnectionIdentity,
    connected: ConnectedServer,
    last_used_at: Instant,
    capabilities_checked_at: Instant,
}

#[derive(Default)]
struct ConnectionPool {
    entries: tokio::sync::Mutex<HashMap<String, CachedConnection>>,
    server_locks: tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

#[derive(Clone)]
pub struct ServerConnector {
    servers: ServerRepository,
    vault: Vault,
    pool: Arc<ConnectionPool>,
}
```

`ServerConnector::new` creates one pool. All connector clones share it. `connect(server_id)` performs these operations in order:

1. Load the current profile, trusted host key, credential payload, and redactor.
2. Build `ConnectionIdentity` without including secret bytes.
3. Acquire the per-server lock, then re-check the pool.
4. Reuse an identity-matching, open transport whose idle age is at most 90 seconds.
5. Re-probe capabilities on the reused transport only when the 10-minute capability TTL expired.
6. Otherwise authenticate once, probe once, and insert the new connection.
7. Update `last_used_at` on each successful lease.

Never hold the global entries lock while awaiting network I/O. The per-server lock may be held across one connection/probe so concurrent callers share the result; callers for other servers remain independent.

- [ ] **Step 4: Add explicit invalidation and shutdown**

Expose:

```rust
pub async fn invalidate(&self, server_id: &str) {
    if let Some(entry) = self.pool.entries.lock().await.remove(server_id) {
        entry.connected.session.disconnect().await;
    }
}

pub async fn shutdown(&self) {
    let entries = std::mem::take(&mut *self.pool.entries.lock().await);
    for (_, entry) in entries {
        entry.connected.session.disconnect().await;
    }
}
```

`connect_at_verified_ip` remains an uncached one-shot connection because it is part of network-change recovery and does not share the configured host identity.

- [ ] **Step 5: Verify GREEN**

Run the Step 2 command. Expected: all connector policy tests pass.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/services/server_connector.rs src-tauri/src/services/mod.rs
git commit -m "feat: pool trusted SSH sessions per server"
```

## Task 3: Reuse one connector throughout application services

**Files:**

- Modify: `src-tauri/src/services/app_services.rs`
- Modify: `src-tauri/src/services/execution_service.rs`
- Modify: `src-tauri/src/services/log_service.rs`
- Modify: `src-tauri/src/services/operation_service.rs`
- Modify: `src-tauri/src/services/operation_restore_service.rs`
- Modify: `src-tauri/src/services/remote_recovery_service.rs`
- Modify: `src-tauri/src/services/restore_point_service.rs`
- Modify: `src-tauri/src/services/task_remediation_service.rs`
- Modify: `src-tauri/src/services/transfer_service.rs`
- Modify: `src-tauri/src/services/workflow_service.rs`
- Modify: `tests/fixtures/sshd/server.py`
- Modify: `src-tauri/tests/m2_live.rs`

- [ ] **Step 1: Instrument the fixture and add a failing live assertion**

In the fixture, count authenticated transports without logging usernames or credentials:

```python
class FixtureServer(asyncssh.SSHServer):
    def connection_made(self, conn: asyncssh.SSHServerConnection) -> None:
        current = int(read_optional_state("connection-count", "0") or "0")
        write_state("connection-count", str(current + 1))
```

Add a helper to read the state file from the project-local fixture root. In `m2_live.rs`, record the count, call `list_remote_directory` for `/` and `/tmp`, then run one safe task on the same server. Assert that these three operations add one transport, not three.

- [ ] **Step 2: Verify RED against the live fixture**

Run:

```powershell
. .\scripts\dev-env.ps1 -Quiet
.\scripts\ssh-fixture.ps1 -Action Start -SkipPythonDependencyInstall
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test m2_live -- --ignored --nocapture --test-threads=1
.\scripts\ssh-fixture.ps1 -Action Stop
```

Expected: the new connection-count assertion fails because v0.1.11 reconnects for each operation.

- [ ] **Step 3: Store and reuse the shared connector**

Add `connector: ServerConnector` to `AppServices`. Construct it once in `open_with_protector`, pass clones to every service, and use `self.connector.connect(...)` in `list_remote_directory` instead of constructing another connector.

Remove normal-path `connected.session.disconnect().await` calls from the listed services. A dropped `ConnectedServer` releases only its clone; the pool controls transport lifetime. Preserve explicit disconnects in host-key inspection and the uncached `connect_at_verified_ip` recovery path.

When server connection settings or credentials are changed by an existing or future command, call `connector.invalidate(server_id)` after the repository update. On application shutdown and before data-root migration, call `connector.shutdown()` after active work is idle.

- [ ] **Step 4: Verify GREEN**

Repeat the Step 2 command inside `try/finally` using `scripts/test-ssh-live.ps1` or manually stop the fixture on failure. Expected: the live assertion passes and the M2 loop remains green.

- [ ] **Step 5: Run focused non-live service tests**

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test execution_services
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test workflow_io_nodes
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_service_integration
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/services src-tauri/tests/m2_live.rs tests/fixtures/sshd/server.py
git commit -m "feat: reuse SSH sessions across application services"
```

## Task 4: Fresh/stale directory cache and obsolete-response protection

**Files:**

- Modify: `src/features/file-browser/directorySessionCache.ts`
- Modify: `src/features/file-browser/directorySessionCache.test.ts`
- Modify: `src/features/transfers/FileTransferPage.tsx`
- Modify: `src/features/transfers/FileTransferPage.test.tsx`

- [ ] **Step 1: Add failing cache tests with a fake clock**

Change the constructor to accept a clock while preserving current callers:

```ts
const clock = vi.fn(() => 1_000);
const cache = new DirectorySessionCache(128, clock, 5_000);
```

Prove that:

- a listing younger than five seconds is fresh and avoids the loader;
- a stale listing is returned by `peekRemote` while `refreshRemote` replaces it;
- two stale refreshes share one promise;
- clearing one server removes its listings and generation state;
- `loadRemote` begun for `/slow` cannot be accepted after `/fast` becomes the current request generation.

- [ ] **Step 2: Verify RED**

```powershell
pnpm test -- --run src/features/file-browser/directorySessionCache.test.ts src/features/transfers/FileTransferPage.test.tsx
```

Expected: tests fail because entries have no timestamp/fresh state and `FileTransferPage` accepts late responses.

- [ ] **Step 3: Implement cache metadata**

Store:

```ts
interface CachedDirectory {
  listing: DirectoryListing;
  fetchedAt: number;
}
```

Expose `freshRemote`, `freshLocal`, and existing `peek*` methods. `load*` returns a fresh cached value, while callers explicitly start a background refresh for stale values. Keep 128-entry LRU and in-flight de-duplication.

- [ ] **Step 4: Guard component requests by generation**

Keep one `useRef(0)` per pane. Increment before every path request and only update listing, error, selection, loading, or refreshing state when the captured generation still matches. A late `/slow` response must populate the reusable cache but must not replace the visible `/fast` path.

- [ ] **Step 5: Verify GREEN**

Run the Step 2 command. Expected: all focused tests pass with no unhandled promise warnings.

- [ ] **Step 6: Commit**

```powershell
git add src/features/file-browser src/features/transfers
git commit -m "fix: make SFTP directory refresh stale-safe"
```

## Task 5: Verification policies and remote fixed-command hashing

**Files:**

- Modify: `src-tauri/src/core/sftp/transfer.rs`
- Modify: `src-tauri/src/core/sftp/mod.rs`
- Modify: `src-tauri/src/services/transfer_service.rs`
- Modify: `src-tauri/src/core/ssh/transport.rs`
- Modify: `src-tauri/tests/sftp_paths.rs`
- Modify: `src-tauri/tests/sftp_live.rs`
- Modify: `src/api/contracts.ts`
- Modify: `src/api/preview.ts`
- Modify: `src/api/preview.test.ts`

- [ ] **Step 1: Add failing policy and parser tests**

Define the intended public types in tests:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationPolicy {
    Balanced,
    Strict,
    TransportOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationLevel {
    RemoteHash,
    TransportAndSize,
}
```

Tests must prove:

- `parse_sha256_output` accepts exactly one leading 64-hex digest followed by whitespace and a path;
- it rejects short, non-hex, extra-prefix, or multiline ambiguous output;
- `remote_hash_command` uses `sha256sum --` plus `tasks::shell_quote(path)` only when capabilities contain `sha256sum`;
- balanced falls back to transport-and-size without a second SFTP read;
- strict selects the existing SFTP re-read fallback when no fixed remote hash capability exists;
- transport-only never requests a remote re-read.

- [ ] **Step 2: Verify RED**

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test sftp_paths
```

Expected: compilation fails because verification types and helpers do not exist.

- [ ] **Step 3: Add policy fields with backward-compatible defaults**

Add `verification: VerificationPolicy` to `UploadRequest` and `DownloadRequest` with `#[serde(default)]`; implement `Default` as `Balanced`. Update every Rust struct literal and TypeScript preview request to pass `balanced` explicitly.

Extend `TransferOutcome` with:

```rust
pub verification_level: VerificationLevel,
pub remote_hash_compared: bool,
```

Keep `sha256` as the receiving/local stream hash so existing history remains meaningful.

- [ ] **Step 4: Execute a safe remote hash command**

Add a helper that uses `execute_authenticated` on the already pooled transport. Only choose commands from `SystemCapabilities`; never accept a command name from the UI. For the current POSIX model:

```rust
format!("sha256sum -- {}", shell_quote(remote_path))
```

Require exit code zero and parse the first field using the strict parser. Redact errors through the existing service redactor before persistence.

For upload, close/fsync the temporary remote file before requesting its hash. For download, request the remote hash before transfer only when the strategy is `RemoteHash`; otherwise stream immediately.

- [ ] **Step 5: Update transfer service and live fixture assertions**

Pass `connected.capabilities` to upload/download. Add a fixture counter for commands beginning with `sha256sum --`. The live roundtrip must prove one remote hash command per direction and no SFTP hash pre-read in balanced mode. Keep a strict-mode test that exercises the fallback with capabilities excluding `sha256sum`.

- [ ] **Step 6: Verify GREEN**

Run:

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test sftp_paths
.\scripts\test-ssh-live.ps1 -SkipPythonDependencyInstall
pnpm test -- --run src/api/preview.test.ts
```

Expected: path/policy, live SFTP, and preview tests pass.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/core/sftp src-tauri/src/core/ssh/transport.rs src-tauri/src/services/transfer_service.rs src-tauri/tests/sftp_paths.rs src-tauri/tests/sftp_live.rs src/api
git commit -m "perf: avoid duplicate SFTP hash reads"
```

## Task 6: Deterministic progress throttling, speed, ETA, and phases

**Files:**

- Modify: `src-tauri/src/domain/events.rs`
- Create: `src-tauri/src/core/sftp/progress.rs`
- Modify: `src-tauri/src/core/sftp/mod.rs`
- Modify: `src-tauri/src/core/sftp/transfer.rs`
- Modify: `src/api/contracts.ts`
- Modify: `src/api/tauri.test.ts`
- Modify: `src/features/transfers/FileTransferPage.tsx`
- Modify: `src/features/transfers/FileTransferPage.test.tsx`

- [ ] **Step 1: Write failing deterministic tracker tests**

Use supplied millisecond timestamps, not sleeps:

```rust
#[test]
fn throttles_to_ten_hertz_and_always_emits_completion() {
    let mut tracker = TransferProgressTracker::new(Some(1_000), 0);
    assert!(tracker.sample(100, 10).is_some());
    assert!(tracker.sample(200, 50).is_none());
    assert!(tracker.sample(300, 110).is_some());
    assert!(tracker.sample(1_000, 111).is_some());
}
```

Also prove EWMA speed stays finite, ETA is absent before a valid sample, total zero cannot divide by zero, and transferred bytes never decrease.

- [ ] **Step 2: Verify RED**

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --lib core::sftp::progress::tests
```

Expected: compilation fails because the module does not exist.

- [ ] **Step 3: Implement progress snapshots**

Define:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferPhase {
    Connecting,
    Transferring,
    Verifying,
    Finalizing,
}

pub struct ProgressSnapshot {
    pub transferred: u64,
    pub total: Option<u64>,
    pub percent: Option<f64>,
    pub bytes_per_second: Option<f64>,
    pub average_bytes_per_second: Option<f64>,
    pub eta_seconds: Option<u64>,
}
```

Use a 100 ms emission interval and EWMA alpha 0.25. The first sample, phase changes, completion, and terminal events always emit.

- [ ] **Step 4: Extend event contracts**

Add optional speed/ETA fields and a required `phase` to Rust and TypeScript progress events. Preview mode emits realistic deterministic values. Preserve monotonic `sequence` handling.

- [ ] **Step 5: Render stable transfer metrics**

Remove the frontend `startedAt` division. Display backend EWMA as “当前速度”, average as “平均速度”, and ETA as “预计剩余”. Show the current phase in the status header and keep the last valid speed during short sample gaps.

- [ ] **Step 6: Verify GREEN**

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --lib core::sftp::progress::tests
pnpm test -- --run src/api/tauri.test.ts src/features/transfers/FileTransferPage.test.tsx
```

Expected: all progress and UI tests pass.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/domain/events.rs src-tauri/src/core/sftp src/api src/features/transfers
git commit -m "feat: report stable SFTP speed and ETA"
```

## Task 7: Tuned upload writes and bounded pipelined downloads

**Files:**

- Create: `src-tauri/src/core/sftp/pipeline.rs`
- Modify: `src-tauri/src/core/sftp/mod.rs`
- Modify: `src-tauri/src/core/sftp/transfer.rs`
- Modify: `src-tauri/tests/sftp_live.rs`

- [ ] **Step 1: Write failing pipeline planner tests**

The planner is pure and must prove:

```rust
#[test]
fn plans_bounded_chunks_without_crossing_file_size() {
    let chunks = plan_window(900_000, 262_144, 8);
    assert_eq!(chunks[0], Chunk { offset: 0, len: 262_144 });
    assert_eq!(chunks.last().unwrap().offset + chunks.last().unwrap().len as u64, 900_000);
    assert!(chunks.len() <= 8);
    assert!(chunks.iter().map(|chunk| chunk.len as usize).sum::<usize>() <= 16 * 1024 * 1024);
}
```

Also test empty files, 64 KiB server limits, 2 MiB client cap, window 1, overflow boundaries, and out-of-order completion draining in ascending offset order.

- [ ] **Step 2: Verify RED**

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --lib core::sftp::pipeline::tests
```

Expected: compilation fails because `pipeline` does not exist.

- [ ] **Step 3: Configure uploads from negotiated limits**

Open high-level SFTP sessions with `russh_sftp::client::Config`:

```rust
Config {
    max_packet_len: 2 * 1024 * 1024,
    max_concurrent_writes: 8,
    request_timeout_secs: ssh.timeout().as_secs().max(1),
}
```

Let `limits@openssh.com` reduce packet sizes. Increase the local transfer buffer from 64 KiB to 256 KiB; high-level `File::poll_write` already keeps at most eight write acknowledgements in flight.

- [ ] **Step 4: Implement offset-read download pipeline**

Use one `RawSftpSession` on a dedicated SFTP channel. Initialize protocol v3, apply `limits@openssh.com` when advertised, open one read handle, and issue at most eight concurrent `read(handle, offset, len)` requests through `tokio::task::JoinSet`. Store completed chunks in `BTreeMap<u64, Vec<u8>>`; write and hash only the next contiguous offset. Never buffer more than the current window or 16 MiB.

On cancellation: abort the join set, close the remote handle/channel, close the local file, and let existing cleanup remove the owned `.partial`. On short reads: schedule the remaining portion at the next offset. On EOF before metadata size: return `AppError::Integrity`.

- [ ] **Step 5: Add a large high-latency live test mode**

The live SFTP test accepts `QZ_SFTP_LARGE_TEST_BYTES`, defaulting to 16 MiB for manual runs. Generate bytes in bounded chunks, upload, download, compare SHA-256, assert progress event count is below `duration_ms / 50 + 4`, and assert no single pipeline window exceeds 16 MiB using test-visible stats.

- [ ] **Step 6: Verify GREEN**

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --lib core::sftp::pipeline::tests
$env:QZ_SFTP_LARGE_TEST_BYTES='16777216'
.\scripts\test-ssh-live.ps1 -SkipPythonDependencyInstall
```

Expected: planner tests and live roundtrip pass; the live output records bytes, transfer milliseconds, and verification level.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/core/sftp src-tauri/tests/sftp_live.rs
git commit -m "perf: pipeline bounded SFTP downloads"
```

## Task 8: Reproducible benchmark and regression gate

**Files:**

- Create: `scripts/benchmark-sftp.ps1`
- Create: `scripts/tests/sftp-performance.tests.ps1`
- Modify: `scripts/ssh-fixture.ps1`
- Modify: `package.json`
- Create: `docs/performance/sftp-benchmark.md`

- [ ] **Step 1: Write a failing contract test for the benchmark script**

The test requires the script to:

- accept `-RttMs`, `-BandwidthMbps`, `-PayloadBytes`, `-Iterations`, and `-OutputPath`;
- reject negative RTT, non-positive bandwidth/payload/iterations, and output outside `artifacts/benchmarks`;
- always stop the fixture/network shaper in `finally`;
- emit JSON containing version, commit, platform, architecture, RTT, bandwidth, payload, iteration samples, median directory time, upload/download throughput, verification policy, CPU, peak memory, progress event count, and cancellation latency;
- never write to AppData or a drive root.

- [ ] **Step 2: Verify RED**

```powershell
& .\scripts\tests\sftp-performance.tests.ps1
```

Expected: failure because the benchmark script and contract do not exist.

- [ ] **Step 3: Implement the benchmark runner**

Use the project-local fixture. On Windows without an available traffic shaper, run loopback measurements and mark `networkShape: unavailable` instead of claiming RTT-limited results. When the CI environment provides the Linux fixture container with `tc netem`, apply the requested RTT/bandwidth there. Warm up once, record at least three measured iterations, and calculate the median without discarding failed samples.

Write only to the validated output path under `artifacts/benchmarks`. Return non-zero if any correctness check fails, even when throughput is high.

- [ ] **Step 4: Add commands and documentation**

Add package scripts:

```json
"test:sftp-performance-contract": "powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/sftp-performance.tests.ps1",
"benchmark:sftp": "powershell -NoProfile -ExecutionPolicy Bypass -File scripts/benchmark-sftp.ps1"
```

Document the exact baseline and comparison commands. State that the design thresholds require shaped Linux CI evidence; local unshaped loopback results are diagnostic only.

- [ ] **Step 5: Verify GREEN**

```powershell
pnpm run test:sftp-performance-contract
pnpm run benchmark:sftp -- -PayloadBytes 16777216 -Iterations 3 -OutputPath artifacts/benchmarks/local-sftp.json
```

Expected: contract passes, benchmark correctness passes, and JSON is produced under the project artifact directory.

- [ ] **Step 6: Commit**

```powershell
git add scripts package.json docs/performance/sftp-benchmark.md
git commit -m "test: add reproducible SFTP performance benchmark"
```

## Task 9: Full verification and documentation alignment

**Files:**

- Modify: `README.md`
- Modify: `docs/user-guide.md`
- Modify: `docs/support-matrix.md`
- Modify: `docs/security.md`
- Modify: `scripts/tests/public-docs.tests.ps1`

- [ ] **Step 1: Add documentation contract failures**

Require public docs to describe session reuse, balanced/strict verification, no duplicate default download pre-read, queue-ready progress phases, and the fact that Windows remains the only released client until later platform plans finish. This prevents this performance phase from falsely claiming the full cross-platform goal is complete.

- [ ] **Step 2: Update documentation from verified behavior**

Document:

- pooled transports still validate the pinned host key on every newly established transport;
- independent channels isolate tasks and transfers;
- balanced verification uses a fixed remote hash command when available and otherwise labels transport-and-size verification;
- strict verification may perform a second remote read;
- directory cache may show stale rows during refresh;
- benchmark reports and limitations.

- [ ] **Step 3: Run the verification ladder**

```powershell
. .\scripts\dev-env.ps1 -Quiet
$env:CARGO_BUILD_JOBS='1'
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --lib
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --tests
pnpm test -- --reporter=dot --maxWorkers=4
pnpm build
& .\scripts\tests\public-docs.tests.ps1
& .\scripts\tests\sftp-performance.tests.ps1
.\scripts\test-ssh-live.ps1 -SkipPythonDependencyInstall
```

Expected: every command passes. If `cargo test --tests` remains over five minutes, record per-binary timings and split CI jobs; do not report success from a timeout.

- [ ] **Step 4: Run security and workspace scans**

Search tracked files, artifacts, and test data for credential/script canaries and forbidden AppData writes. Confirm `git status --short` contains only intended changes and benchmark JSON remains ignored unless explicitly selected as evidence.

- [ ] **Step 5: Commit**

```powershell
git add README.md docs scripts/tests/public-docs.tests.ps1
git commit -m "docs: explain faster verified SFTP behavior"
```

## Execution mode

The user explicitly requested continuous execution without further questions. Execute this plan inline with `superpowers:executing-plans`, use `superpowers:test-driven-development` for every behavior change, and use `superpowers:verification-before-completion` before claiming this subproject complete. Continue automatically to the next design subproject after this plan passes its completion gate.
