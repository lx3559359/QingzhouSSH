# QingzhouSSH Delivery Roadmap

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement each milestone plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver QingzhouSSH as a safe, local-only Windows desktop application in four independently testable milestones.

**Architecture:** Keep React/TypeScript in an untrusted presentation layer and route all filesystem, credential, SSH/SFTP, workflow, and updater actions through typed Tauri commands into focused Rust services. Each milestone must leave a runnable application and stable interfaces for the next milestone.

**Tech Stack:** Tauri 2, React, TypeScript, Rust, SQLite via SQLx, `ssh2`/libssh2 with vendored OpenSSL, Windows DPAPI, Vitest, Rust tests, Docker-based SSH integration fixtures.

---

## Plan sequence

### Milestone 1: Secure connection foundation

Detailed plan: `docs/superpowers/plans/2026-08-03-qingzhou-ssh-m1-foundation.md`

Delivers:

- project-local D-drive development environment;
- runnable Tauri shell and visual tokens;
- user-selected/portable data root;
- incognito first-run WebView and selected-root WebView2 cache;
- SQLite migrations and server repository;
- DPAPI credential vault;
- SSH host-key inspection and trust flow;
- password/private-key authentication;
- Linux family and capability probe;
- first-run and add-server UI;
- live SSH fixture and D-drive path audit.

Gate: a clean Windows machine can select a data root, add a Linux server, approve its fingerprint, authenticate, and display detected system capabilities without writing application or persistent WebView2 data to default AppData.

### Milestone 2: Quick tasks, logs, and file transfer

Plan is written after Milestone 1 so it can use the actual repository, error, transport, and IPC interfaces without guessing.

Delivers:

- versioned task-definition schema and compatibility predicates;
- system status, service management, and log-search templates;
- typed parameter validation and shell escaping;
- streaming command output through Tauri channels;
- SFTP upload/download with SHA-256 verification;
- `.log` and `.gz` search adapters;
- paged preview, result download, and redacted execution history;
- advanced custom-command and multi-line-script editor.

Gate: A+B+C task categories complete their full UI → validation → SSH/SFTP → structured result → history/download cycle on every supported system family.

### Milestone 3: Workflow engine and recovery

Plan is written after Milestone 2 so workflow nodes wrap proven task and transfer APIs instead of duplicating them.

Delivers:

- persisted workflow graph and schema migrations;
- linear execution plus bounded conditional branches;
- node validation, state machine, retries, and cancellation;
- controlled remote process groups and uncertain-cancellation reporting;
- restore-point creation, rollback, and lifecycle cleanup;
- workflow editor, parameter panel, execution timeline, and diagnostics;
- crash recovery that marks interrupted runs as remotely unconfirmed.

Gate: the reference deployment workflow can fail at every node and always produce an accurate, explainable, recoverable state.

### Milestone 4: Packaging, dual-source updates, and public release

Plan is written after Milestone 3 so packaging captures the real data migrations, binaries, and release artifacts.

Delivers:

- portable and installer distributions;
- signed Tauri update artifacts and SHA-256 manifests;
- user-confirmed update UX;
- GitHub Releases primary source and ModelScope mirror fallback;
- same-build artifact synchronization and mirror validation;
- Apache-2.0 license, security policy, support matrix, and user guide;
- release checks for startup time, memory, package size, upgrade, rollback, and clean uninstall.

Gate: a signed release installs and upgrades on a clean Windows reference machine from either update source, while rejecting modified or mismatched artifacts.

## Cross-milestone rules

- Use test-driven development for each behavior: failing test, minimal implementation, passing test, then commit.
- Keep project files focused; transport, storage, domain, command, and UI responsibilities must not share catch-all modules.
- Never expose a filesystem path, secret, raw SSH session, or database handle directly to the WebView.
- Never log credential material. Tests include known-secret canaries and fail if any canary appears in output.
- Keep all controllable development dependencies, caches, targets, test data, and artifacts under `D:\Codex Project\轻量化SSH快捷工具`.
- Do not begin a milestone until the previous gate passes and the user accepts the checkpoint.
- Author the next milestone plan against the checked-in code at that gate; do not preserve speculative APIs merely to match this roadmap.
