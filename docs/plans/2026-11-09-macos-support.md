# macOS support

## Decision

`tb` builds and runs on **macOS on Apple Silicon** (`aarch64-apple-darwin`). Intel Macs are optional, a second
build target once Apple Silicon works.

- **How:** a small `os` module in `crates/tb` gathers everything that differs between Linux and macOS. Linux
  keeps today's code; macOS gets its own versions through the `libc` crate, which is already in `Cargo.lock`.
- **Size of the job:** the compile check found only **two** compile errors on macOS; everything else
  type-checks, tests included ([finding](../findings/2026-11-09-macos-port-survey.md)). Four `/proc` reads and
  the trash command compile but don't work there.
- **Effort (estimate):** 1–2 days of code, plus CI and checking it on a real Mac.

### Example: a tab name on a Mac

You open `tb` in iTerm2 and type `cd ~/docs`. The tab bar should show `docs`, as on Linux
([tab names follow the folder](2026-11-09-tab-name-folder.md)).

```
 tb server (macOS)                                   macOS kernel
┌────────────────────────────────┐                 ┌───────────────────────────────────┐
│ draw_tab_bar                   │ 1 pid 4242      │ proc_pidinfo(4242,                │
│   pane.cwd()                   │────────────────▶│   PROC_PIDVNODEPATHINFO)          │
│     os::process_cwd(4242)      │                 │   -> pvi_cdir = /Users/u/docs     │
│       (macos.rs)               │◀────────────────│                                   │
│   folder_label -> "docs"       │ 2 path          └───────────────────────────────────┘
└────────────────────────────────┘
```

Today, step 1 reads `/proc/4242/cwd`, which doesn't exist on macOS. The tab falls back to "shell".

### Where the OS-specific code goes

```
 crates/tb/src/
┌────────────────────────┐   calls   ┌──────────────────────────────────────────────┐
│ client.rs  (tb kill)   │──────────▶│ os/mod.rs                                    │
│ server/mod.rs          │           │   peer_cred(&UnixStream) -> Peer {uid, pid}  │
│   (same-user check)    │           │   process_cwd(pid)   process_name(pid)       │
│ server/pane.rs         │           │   process_args(pid)  default_trash_command() │
│   (cwd, program names) │           ├──────────────────────┬───────────────────────┤
│ finder/menu.rs (trash) │           │ os/linux.rs          │ os/macos.rs           │
└────────────────────────┘           │ SO_PEERCRED (rustix) │ getpeereid +          │
                                     │ /proc/<pid>/cwd,     │   LOCAL_PEERPID       │
                                     │   comm, cmdline      │ proc_pidinfo,         │
                                     │ gio trash --         │   proc_name,          │
                                     │                      │   sysctl PROCARGS2    │
                                     │                      │ trash (macOS 15+)     │
                                     └──────────────────────┴───────────────────────┘
```

## Options considered

- **`libc` directly, behind `#[cfg(target_os = …)]` (chosen):** `libc` 0.2.191 already has every call we need
  and is in the lock file, so it builds offline and adds no new license.
- **The `libproc` crate:** wraps `proc_pidinfo` and the others more neatly, but it's a new dependency
  (`cargo fetch`, a license check), and the peer check still needs `libc`.
- **Skip the features on macOS** (no folder-following tab names, no "end an old server"): least work, but
  tab names and session restore would quietly behave worse on a Mac.
- **Wait for `UnixStream::peer_cred()` in Rust's standard library:** still unstable, so not usable now.

## Steps

- [x] Survey the code and type-check it for macOS
  ([finding](../findings/2026-11-09-macos-port-survey.md)).
- [ ] **`os` module.**
  - Move the Linux code from `pane.rs:143-174`, `client.rs:234` and `server/mod.rs:246` into `os/linux.rs`
    unchanged. Then write `os/macos.rs`.
  - Add `libc` as a direct dependency.
  - The same-user check in `server/mod.rs` must still refuse other users on macOS. Don't drop it.
- [ ] **Unit tests that run on both systems:** for the test process itself, check that
  - `process_cwd` equals `env::current_dir()`
  - `process_name` is the test binary's name
  - `process_args` starts with its path
  - `peer_cred` on a `UnixStream::pair()` gives our own uid and pid
- [ ] **Trash on macOS:** default to `trash` and check it exists on macOS 15+ (**estimate**, not verified).
  Fall back to telling the user to set `$TB_TRASH`. Its error text differs from gio's, so check the message
  we show.
- [ ] **Install hints:** the PDF message names `brew install poppler` on macOS instead of `poppler-utils`
  (`finder-core/src/pdf.rs:43`).
- [ ] **CI:**
  - On Linux (cheap), add `cargo check --target aarch64-apple-darwin` and the same clippy run, so Linux-only
    calls are caught on every push.
  - On a `macos-14` runner, run `cargo test --workspace` with `brew install poppler` first. That runs the e2e
    tests for real. macOS minutes cost more on private repos.
- [ ] **On a real Mac, by hand:**
  - Images in iTerm2 and Ghostty: does `poll()` on stdin work for the terminal query (`client.rs:425`)? If not,
    use `select()`.
  - The Option key as Alt (`Alt+s`, `Alt+q` in the finder).
  - OSC 52 copy in iTerm2.
  - Trash.
  - Tab names follow `cd`.
  - Restore after `tb kill`.
- [ ] **Docs:** a macOS section in `README.md` covering the terminal settings (Option as Alt, iTerm2 clipboard
  permission, which terminals show images) and `brew install poppler neovim`.
- [ ] **Release builds:** add `aarch64-apple-darwin` to the release matrix in the
  [distribution plan](2026-11-09-distribution.md).

## Open questions

- Is there a Mac to test on, or do we rely on the macOS CI runner plus someone else's Mac?
- Intel Macs: worth a second target? **Estimate:** Apple's last macOS for Intel is 26, so the audience is
  shrinking.
- The [rebuild plan](2026-11-03-rebuild-plan.md)'s open question 4 ("Linux only at launch, or macOS too?")
  should be answered there once this is agreed.
