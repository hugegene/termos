# macOS support

## Decision

`tb` builds and runs on **macOS on Apple Silicon** (`aarch64-apple-darwin`). **Linux and macOS both ship at
launch.** Intel Macs are not supported.

*Changed 2026-11-10:* the command is now **`tos`** (product **termos**), so read `tb` below as `tos`
([rename plan](2026-11-10-rename-to-termos.md)).

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
- [x] **`os` module** (2026-11-10): `crates/tb/src/os/{mod,linux,macos}.rs`.
  - The Linux code from `pane.rs`, `client.rs` and `server/mod.rs` moved to `os/linux.rs` unchanged.
    `os/macos.rs` uses `libc`, a dependency only when building for macOS.
  - The same-user check in `server/mod.rs` still refuses other users, through `os::peer_cred` on both
    systems.
  - Any other system fails to compile with "tb runs on Linux and macOS only".
- [x] **Unit tests that run on both systems** (in `os/mod.rs`): for the test process itself,
  - `process_cwd` equals `env::current_dir()`
  - `process_name` is a prefix of the test binary's name, because Linux cuts it to 15 bytes
  - `process_args` equals `env::args()`
  - a pid that doesn't exist gives `None` for all three
  - `peer_cred` gives our own uid and pid on both ends of a socket.
    - *Changed:* the plan said `UnixStream::pair()`. The test binds, connects and accepts instead, as the
      server does, because it's unverified whether macOS gives peer credentials for a socket pair.
  - The `KERN_PROCARGS2` parser lives in `os/mod.rs`, so its test runs on Linux too: the environment stays
    out, and empty arguments still count.
- [x] **Trash on macOS:** the default is `trash` (macOS 15+, an **estimate**, still not verified). When the
  default program is missing, the message adds "$TB_TRASH can name another trash command".
  - Still to do on the Mac: does `trash` exist, and does its error text read well after we cut it at the last
    `: `?
- [x] **Install hints:** the PDF message names `brew install poppler` on macOS
  (`finder-core/src/pdf.rs`).
- [x] **CI** (`.github/workflows/ci.yml`). *First run 2026-11-10:* `check` passed. The `macos` job never
  started, because GitHub has retired `macos-14`, so it now runs on `macos-latest`. CI runs:
  - On Linux (cheap), clippy also runs with `--target aarch64-apple-darwin`, so Linux-only calls are caught
    on every push. Clippy includes `cargo check`.
  - A `macos-latest` runner runs `cargo test --workspace` after `brew install poppler`, which runs the e2e
    tests for real. The repo is public, so macOS minutes are free.
- [ ] **On the user's Mac, by hand:**
  - Images in iTerm2 and Ghostty: does `poll()` on stdin work for the terminal query (`client.rs:425`)? If not,
    use `select()`.
  - The Option key as Alt (`Alt+s`, `Alt+q` in the finder).
  - OSC 52 copy in iTerm2.
  - Trash.
  - Tab names follow `cd`.
  - Restore after `tos kill`.
- [ ] **Docs:** a macOS section in `README.md` covering the terminal settings (Option as Alt, iTerm2 clipboard
  permission, which terminals show images) and `brew install poppler neovim`.
- [x] **Release builds** (2026-11-10): `aarch64-apple-darwin` is in the release matrix
  (`.github/workflows/release.yml`, on `macos-latest`), as the [distribution plan](2026-11-09-distribution.md)
  describes.
  - The first macOS CI runs found two tests that only passed on Linux, both fixed. The PDF test expected
    poppler 24.02's wording, and the trash test used GNU `mv -t`.
- [x] **Shells start without `/usr/bin/login`** (2026-11-10, found by the first macOS CI run of the e2e tests):
  - **The bug:** given no command, `alacritty_terminal` starts the shell on macOS as
    `/usr/bin/login -flp <user> /bin/zsh -fc 'exec -a -<shell> <shell>'`. `login` runs as root, so the pane's
    process was `login`, and `proc_pidinfo` couldn't read its folder. Tabs stayed "shell" instead of following
    `cd`, and restored sessions lost their folders. `running_command` also compared the foreground group
    with `login`'s pid, so it would have taken the idle shell for a command to run again.
  - **The fix:** on macOS, tb starts `$SHELL -l` (else `/bin/zsh -l`) itself (`os::default_shell`), so it's
    still a login shell, as Terminal.app gives. The fork is unchanged. The "Last login: …" line no longer
    appears.
  - **The tests:** the e2e tests use `SHELL=/bin/dash` instead of `/bin/sh`. On macOS `/bin/sh` is bash, and
    bash as a login shell reads `/etc/bashrc`, which replaces the tests' `PS1="$ "`.
- [x] **`~` for a home folder reached through a symlink** (2026-11-10, found by the same CI runs): the kernel
  reports a shell's folder by its real path. On macOS, `/var/…` is really `/private/var/…`, so a tab whose
  shell was in `$HOME` didn't show `~`. `paths::home_dirs()` now gives `$HOME` and its real path, and tab
  labels and the finder's `~/…` use both. This also helps Linux homes that sit behind a symlink.

## Open questions

None at the moment. All three were answered on 2026-11-10; see the change log.

## Change log

- **2026-11-09:** first version.
- **2026-11-10:** the user answered the open questions.
  - **A Mac to test on:** the user has one, so the by-hand checks run there. The CI runner still runs the
    tests on every push.
  - **Intel Macs:** not supported. Only `aarch64-apple-darwin` is built. **Estimate:** macOS 26 is the last
    version for Intel, so that audience is shrinking.
  - **Launch:** Linux and macOS both ship at launch. This answers open question 4 in the
    [rebuild plan](2026-11-03-rebuild-plan.md).
- **2026-11-10:** the `os` module, its tests, the trash and PDF hints and CI are done.
  - **Checked on Linux:** the whole workspace builds and its tests pass, the 7 new tests included. Clippy is
    clean, and the licenses pass.
  - **Checked for macOS, without running it:** `cargo clippy --target aarch64-apple-darwin --all-targets` is
    clean.
  - **Not yet run on macOS:** the macOS CI job runs on the next push. Then come the by-hand checks on the
    user's Mac.
  - One change from the plan: the peer test uses a real socket instead of a socket pair (see Steps).
