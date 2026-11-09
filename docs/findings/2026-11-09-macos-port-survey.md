# Finding: what in `tb` is Linux-only (a code survey for macOS support)

**Date:** 2026-11-09 · **Machine:** Ubuntu 24.04.5 LTS, the dev box · **Code:** `e715574` · **rustix:** 1.1.6 ·
**notify:** 8.2.1

## Question

What would have to change for `tb` to build and run on macOS? This decides the size of a macOS plan, and
answers open question 4 of the [rebuild plan](../plans/2026-11-03-rebuild-plan.md) ("Linux only at launch,
or macOS too?").

This started as a reading of the code. Later the same evening we also type-checked it for macOS (see the
compile check below). Nothing has been linked for macOS or run on a Mac.

## Method

```sh
grep -rn "cfg(.*target_os\|cfg(unix\|cfg(linux" crates/tb crates/finder-core
grep -rnE "/proc/|epoll|inotify|signalfd|eventfd|timerfd|pidfd|memfd|prctl|/dev/shm|/sys/" crates/tb/src crates/finder-core/src
grep -rn "rustix" crates/tb/src crates/finder-core/src
# is each rustix call available on Apple targets?
R=~/.cargo/registry/src/index.crates.io-*/rustix-1.1.6
grep -n -B6 "pub fn renameat_with"   $R/src/fs/at.rs        | grep cfg
grep -n -B4 "pub fn socket_peercred" $R/src/net/sockopt.rs  | grep cfg
# programs we start, and Alt key bindings
grep -rnE '"(gio|xdg-open|nvim|pdftotext|pdftoppm|pdfinfo)"' crates/tb/src crates/finder-core/src
grep -rnc "KeyModifiers::ALT" crates/tb/src
# are notify's macOS dependencies already downloaded (for an offline cargo check)?
grep -n -A1 "target_os=\"macos\"" ~/.cargo/registry/src/index.crates.io-*/notify-8.2.1/Cargo.toml
ls -d ~/.cargo/registry/src/index.crates.io-*/objc2-core-{services,foundation}-0.3.2
rustup target list --installed
```

### Compile check (added later on 2026-11-09)

The user ran `rustup target add aarch64-apple-darwin`. Type-checking needs only that target's standard library,
not a Mac or Apple's linker.

```sh
# 1. the repo as it is
cargo check --offline --target aarch64-apple-darwin -p tb -p finder-core --message-format short

# 2. a scratch copy of HEAD (git archive) with the two failing calls stubbed out, to reveal any errors hidden
#    behind them; --all-targets adds the unit tests and crates/tb/tests/e2e.rs
#    client.rs:234      -> let peer: (Uid, Pid) = { let _ = &stream; unimplemented!() };  (peer.uid/.pid -> .0/.1)
#    server/mod.rs:246  -> { let _ = stream; true }
CARGO_TARGET_DIR=$SCRATCH/mac-target cargo check --offline --target aarch64-apple-darwin \
  -p tb -p finder-core --all-targets --message-format short
```

## Results

No `#[cfg(target_os …)]` anywhere in our two crates.

**Won't compile on macOS:** `socket_peercred` is `#[cfg(linux_kernel)]` in rustix 1.1.6.

```
crates/tb/src/client.rs:234:      let peer = rustix::net::sockopt::socket_peercred(&stream)?;      # tb kill, old servers
crates/tb/src/server/mod.rs:246:  rustix::net::sockopt::socket_peercred(stream).is_ok_and(|c| c.uid == rustix::process::getuid())
```

**Compiles, but does nothing on macOS (no `/proc`):**

```
crates/tb/src/server/pane.rs:143:  std::fs::read_link(format!("/proc/{}/cwd", self.pid)).ok()
crates/tb/src/server/pane.rs:149:  ... format!("/proc/{}/comm", pgid.as_raw_nonzero())
crates/tb/src/server/pane.rs:161:  ... format!("/proc/{}/cmdline", pgid.as_raw_nonzero())
crates/tb/src/server/pane.rs:169:  ... format!("/proc/{}/cmdline", self.pid)
```

**Linux tools as defaults:**

```
crates/tb/src/server/finder/menu.rs:128:  _ => vec!["gio".into(), "trash".into(), "--".into()],
crates/finder-core/src/pdf.rs:43:         format!("PDF preview needs {tool} (package poppler-utils)")
```

**Already portable:**

- `renameat_with(... RenameFlags::NOREPLACE)` (menu.rs:159) is `#[cfg(any(apple, linux_kernel, ...))]`.
- `paths.rs` falls back to `/tmp/tb-<uid>` when `XDG_RUNTIME_DIR` is unset, as on macOS.
- Clipboard is OSC 52 (server/mod.rs:610), with no clipboard program.
- The image protocols are kitty, iTerm2 and sixel (client.rs:352-354).
- The `alacritty_terminal` fork depends only on `cfg(unix)` crates (rustix, rustix-openpty, signal-hook).
- notify 8.2.1 on macOS uses objc2-core-foundation and objc2-core-services 0.3.2. Both are already in the
  local registry, as are kqueue 1.2.1 and libc 0.2.191.

**Needs a check on a Mac:** client.rs:425 calls `poll()` on stdin to read the terminal's answers (image
protocol detection).

**Alt bindings:** `KeyModifiers::ALT` appears in 6 files. Examples: `"Alt+s save · Alt+q close"` (finder) and
`"Alt+n"` (keys.rs:702).

**Installed Rust targets:** `x86_64-unknown-linux-gnu` only at first. `aarch64-apple-darwin` was added later.

### Compile check results

1. The repo as it is (10 s, every dependency including the `alacritty_terminal` fork and notify's macOS
   backend checked fine):

```
crates/tb/src/client.rs:234:46: error[E0425]: cannot find function `socket_peercred` in module `rustix::net::sockopt`
crates/tb/src/server/mod.rs:246:27: error[E0425]: cannot find function `socket_peercred` in module `rustix::net::sockopt`
error: could not compile `tb` (bin "tb") due to 2 previous errors
```

2. With both stubbed (the first stub also hit `rustix::net::UCred`, which is Linux-only too, so it was
   rewritten as the tuple shown above):

```
crates/tb/src/client.rs:235:13: warning: unreachable statement: unreachable statement
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.77s
```

### The macOS calls in `libc` 0.2.191 (already in the lock file, `MIT OR Apache-2.0`)

```sh
L=~/.cargo/registry/src/index.crates.io-*/libc-0.2.191
for s in "fn getpeereid" LOCAL_PEERPID SOL_LOCAL "fn proc_pidinfo" PROC_PIDVNODEPATHINFO \
         "struct proc_vnodepathinfo" "fn proc_name" KERN_PROCARGS2 "fn sysctl"; do
  grep -rln "$s" $L/src/unix/bsd/; done
```

All found. `getpeereid` is in `src/unix/bsd/mod.rs:627` (shared by the BSDs and Apple). The rest are in
`src/unix/bsd/apple/mod.rs`. No new crate is needed for the macOS versions.

## Conclusions

- **Two calls block the build** (peer credentials) and **four `/proc` reads** need macOS versions. The
  likely replacements, **not yet tried**:
  - `getpeereid()` and the `LOCAL_PEERPID` socket option for the peer check
  - `proc_pidinfo(PROC_PIDVNODEPATHINFO)` for the working folder, `proc_name()` for the program name, and
    `sysctl(KERN_PROCARGS2)` for the command line, through `libc` (already in the lock file) or the
    `libproc` crate
- **The trash default** needs a macOS command. **Estimate:** `/usr/bin/trash` ships with macOS 15 and later;
  check it on a Mac.
- **`poll()` on a terminal is a known macOS weak spot.** macOS's man page says `poll()` doesn't support
  devices, and tmux avoids it on macOS. If it fails, the result is "no images" rather than a crash. Test it
  first; `select()` is the fallback.
- **Alt bindings need the terminal's Option key set to send Alt.** In Terminal.app that's "Use Option as Meta
  key"; in iTerm2 it's "Esc+"; in Ghostty it's `macos-option-as-alt`. Without it, Option+s types `ß`.
- **The compile check confirms it:** the peer-credential calls (`socket_peercred` and its `UCred` type) are
  the only compile errors, in `tb` itself and in all its tests. Everything else, the `alacritty_terminal` fork
  included, type-checks for macOS.
- **Still unknown:** anything only linking or running shows: the `/proc` reads, the trash command, `poll()`
  on a terminal, and the e2e tests. That needs a Mac or a macOS CI runner.
