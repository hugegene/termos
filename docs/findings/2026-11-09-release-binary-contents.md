# Finding: what the release binary contains, and where it can run

**Date:** 2026-11-09 · **Machine:** Ubuntu 24.04.5 LTS (Linux 6.8), the dev box · **Toolchain:** rustc 1.99.1,
cargo 1.99.1, GNU binutils 2.42

## Question

Before we hand `tb` to other people as a binary, we needed to know:

- Does the release binary give away our source, or details about the dev machine?
- Which settings would change that (stripping, path remapping)?
- Which machines can run the binary as it's built today?

The answers decide the release profile and the build targets in a future distribution plan.

## Method

`target/release/tb` as built on 2026-11-09 at 23:24 from `e715574` with the repo's `[profile.release]`
(`lto = "thin"`, nothing else).

```sh
file target/release/tb
strings -n 8 target/release/tb | grep -c /home/hugegene          # absolute paths naming the user
strings -n 8 target/release/tb | grep /home/hugegene | sort -u | head -8
strings -n 8 target/release/tb | grep -c crates/tb/src           # paths into our own crate
nm target/release/tb | wc -l                                      # symbol names
nm -C target/release/tb | grep -m5 ' tb::'
readelf -S target/release/tb | grep -c debug                      # DWARF sections
grep -rn 'include_str!\|include_bytes!' crates/tb crates/finder-core

# stripping, on a copy in the scratchpad
cp target/release/tb $SCRATCH/tb-stripped && strip $SCRATCH/tb-stripped
ls -la $SCRATCH/tb-stripped; nm $SCRATCH/tb-stripped
strings -n 8 $SCRATCH/tb-stripped | grep -c /home/hugegene

# where it can run
objdump -T target/release/tb | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -2
objdump -T target/release/tb | grep -E 'pidfd_(spawnp|getpid)'   # which calls need 2.39, and are they weak?
readelf -V target/release/tb | sed -n '/Version needs/,$p' | grep -E 'File:|Name:'   # required vs WEAK
ldd --version | head -1
ldd target/release/tb
grep -rn '/proc/' crates/tb/src crates/finder-core/src
```

## Results

```
target/release/tb: ELF 64-bit LSB pie executable, x86-64, ... dynamically linked, ... for GNU/Linux 3.2.0, not stripped
-rwxrwxr-x 12755920 target/release/tb
```

| Check | As built | After `strip` |
|---|---|---|
| Size | 12,755,920 bytes | 9,625,360 bytes |
| Symbol names (`nm`) | 28,243 | 0 ("no symbols") |
| Strings containing `/home/hugegene` | 272 | 272 |
| Strings containing `crates/tb/src` | 17 | (not re-counted) |
| DWARF debug sections | 0 | 0 |

Sample symbol names (our types are readable):

```
<alacritty_terminal::event_loop::EventLoop<alacritty_terminal::tty::unix::Pty, tb::server::pane::EventProxy>>::pty_read::<std::fs::File>
<hashbrown::map::HashMap<u32, tb::server::Pane, std::hash::random::RandomState>>::get::<u32>
<hashbrown::map::HashMap<u32, tb::server::Client, std::hash::random::RandomState>>::get_mut::<u32>
```

Sample paths (panic locations of dependencies; the same kind exists for our own crates):

```
/home/hugegene/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/aho-corasick-1.1.5/src/ahocorasick.rs
/home/hugegene/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/aho-corasick-1.1.5/src/dfa.rs
```

Files embedded on purpose, readable with `strings`:

```
crates/tb/src/server/editor.rs:13:const NVIM_INIT: &str = include_str!("../../../../nvim/init.lua");
crates/tb/src/main.rs:405:        let doc = include_str!("../../../docs/cli.md");
```

Where it can run:

```
GLIBC_2.34
GLIBC_2.39

0000000000000000  w   DF *UND*	0000000000000000 (GLIBC_2.39) pidfd_spawnp
0000000000000000  w   DF *UND*	0000000000000000 (GLIBC_2.39) pidfd_getpid

libc.so.6 version needs (trimmed; every version not shown is "Flags: none"):
  GLIBC_2.17  Flags: none
  GLIBC_2.18  Flags: WEAK
  GLIBC_2.25  Flags: WEAK
  GLIBC_2.28  Flags: WEAK
  GLIBC_2.29  Flags: WEAK
  GLIBC_2.30  Flags: WEAK
  GLIBC_2.32  Flags: none
  GLIBC_2.33  Flags: none
  GLIBC_2.34  Flags: none
  GLIBC_2.39  Flags: WEAK
libm.so.6: GLIBC_2.27 Flags: none (highest)

ldd (Ubuntu GLIBC 2.39-0ubuntu8.9) 2.39

	libgcc_s.so.1 => /lib/x86_64-linux-gnu/libgcc_s.so.1
	libm.so.6 => /lib/x86_64-linux-gnu/libm.so.6
	libc.so.6 => /lib/x86_64-linux-gnu/libc.so.6
crates/tb/src/server/pane.rs:143:  std::fs::read_link(format!("/proc/{}/cwd", self.pid)).ok()
crates/tb/src/server/pane.rs:149:  ... format!("/proc/{}/comm", ...)
crates/tb/src/server/pane.rs:161:  ... format!("/proc/{}/cmdline", ...)
crates/tb/src/server/pane.rs:169:  ... format!("/proc/{}/cmdline", self.pid)
```

## Conclusions

- **No source code is in the binary.** It's machine code. What it does give away:
  - **Function and type names** (28,243 symbols). `strip = true` in `[profile.release]` removes them and saves
    about 3.1 MB (25 %).
  - **Absolute file paths** with the user name and the folder layout (272 + 17). They come from panic
    locations, so stripping doesn't remove them. Rust's stable `--remap-path-prefix` flag should rewrite them;
    **not yet measured**.
  - **Every string literal**, including the embedded `nvim/init.lua` and `docs/cli.md`. That's intended, but
    anything in those files is public once we ship.
  - Disassembly is always possible. Hiding the code relies on keeping the repo private and the license, not on
    the binary format.
- **It needs glibc 2.34 or newer, not 2.39.** (Corrected the same evening: a first reading took the highest
  version listed, 2.39, as the minimum.)
  - The only 2.39 functions, `pidfd_spawnp` and `pidfd_getpid`, are **weak** references from Rust's standard
    library. On an older glibc the loader leaves them empty, and `std::process::Command` falls back to an
    older way of starting processes.
  - The highest version marked `Flags: none` is GLIBC_2.34, so that's the real minimum.
  - Distro glibc versions below are from memory, **not checked**: it should start on Ubuntu 22.04 (2.35),
    Debian 12 (2.36) and RHEL 9 (2.34), and refuse on Ubuntu 20.04 and Debian 11 (2.31) and RHEL 8 (2.28).
    No older-distro image was available here to try it.
  - Its only shared libraries are libc, libm and libgcc_s, so a fully static `x86_64-unknown-linux-musl`
    build looks possible (**not yet tried**). It would also run on distros older than glibc 2.34.
- **macOS needs code changes:** `pane.rs` reads `/proc/<pid>/{cwd,comm,cmdline}`, which macOS doesn't have.
- **Still unknown:** whether the musl build works and how fast it is (musl's allocator is slower; **estimate**:
  it may matter for heavy output), and whether `--remap-path-prefix` clears all 289 paths.

## Added later on 2026-11-09: where the absolute paths come from

**Question:** do we need `--remap-path-prefix` for releases? It depends on which paths are absolute.

**Method:**

```sh
B=target/release/tb
strings -n 8 $B | grep -c '/home/hugegene/.cargo/registry'
strings -n 8 $B | grep -c '/home/hugegene/.rustup'
strings -n 8 $B | grep -c '/home/hugegene/terminal-browser'
strings -n 8 $B | grep '/home/hugegene' | grep -vc '/home/hugegene/.cargo/registry'
strings -n 8 $B | grep -c '/rustc/'
strings -n 8 $B | grep -oE '[^ ]*crates/(tb|finder-core|alacritty_terminal)/src/[^ ]*\.rs' | sort -u | head -8
```

**Results:**

```
registry: 272
rustup:   0
repo abs: 0
other /home/hugegene: 0
std:      96
crates/alacritty_terminal/src/event_loop.rs
crates/alacritty_terminal/src/grid/mod.rs
crates/alacritty_terminal/src/grid/resize.rs
...
```

**Conclusions:**

- All 272 absolute paths are dependency sources under `~/.cargo/registry`. They show only the build machine's
  home folder and each dependency's name and version.
- Our own crates are already relative (`crates/tb/src/…`). Rust's standard library is already remapped
  (`/rustc/<hash>/…`).
- Built in CI, the home folder would be the runner's (from memory, `/home/runner`), not a person's. The
  dependency names and versions are listed in `THIRD_PARTY_NOTICES.md` anyway.
- **So `--remap-path-prefix` isn't needed for CI-built releases.** It would only hide the user name on a
  release built on the dev box, which the [distribution plan](../plans/2026-11-09-distribution.md) doesn't do.
- The relative paths of our own files are worth keeping. A panic message such as `panicked at
  crates/tb/src/server/pane.rs:143` makes a user's bug report useful.
