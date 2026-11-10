# Upgrading installed copies: a versions folder, `tos update`, and a notice

## Decision

Copy Claude Code's layout, but don't update automatically yet. There are three parts:

1. **A versions folder:** `install.sh` puts each version in `~/.local/share/termos/versions/<version>/tos`
   and points the symlink `~/.local/bin/tos` at it. It keeps the newest two versions.
2. **`tos update`:** it runs the installer for the user (`tos update`, or `tos update --version 0.2.0` to roll
   back).
3. **A notice:** at most once a day, attaching checks the latest release. If there's a newer one, the status
   bar says `tos 0.2.2 is available: tos update`. `TOS_NO_UPDATE_CHECK=1` turns this off.

Nothing updates itself in the background. A multiplexer that swaps its binary under a running session runs
into the protocol check (`PROTOCOL_VERSION`, now 3). A notice plus a command leaves the timing to the user.
Background updates can come later, once upgrades between versions are tested.

### Today, for comparison

Upgrading means re-running the one-liner. `install.sh` copies the new binary next to `~/.local/bin/tos` and
renames it over the old one. No old version is kept, and `tos` never learns that a newer one exists.

Claude Code, as installed on this machine on 2026-11-10 (looked at, not measured further):

```
~/.local/bin/claude ──symlink──▶ ~/.local/share/claude/versions/2.1.322   (342 MB)
                                 ~/.local/share/claude/versions/2.1.317 … 2.1.321   (1.6 GB in all)
```

### Example: 0.2.1 installed, 0.2.2 released

```
 tos client (attach)                   github.com                       ~/.local/share/termos/
┌────────────────────┐ 1 HEAD /releases/latest  ┌──────────────┐
│ last check > 1 day?│ ───────────────────────▶ │ 302 → …/tag/ │
│                    │ ◀─────────────────────── │   v0.2.2     │
│ 2 0.2.2 > 0.2.1:   │                          └──────────────┘
│   status bar note  │
└────────────────────┘
 user runs `tos update`
┌────────────────────┐ 3 install.sh, tarball, .sha256 ┌──────────────┐   4 versions/0.2.2/tos
│ tos update         │ ─────────────────────────────▶ │ release      │ ──▶ symlink ~/.local/bin/tos
│ (runs install.sh)  │                                │ assets       │     → versions/0.2.2/tos
└────────────────────┘                                └──────────────┘   5 deletes all but 0.2.2, 0.2.1
```

1. On attach, if `~/.local/state/termos/update-check` is more than a day old, the client asks for
   `https://github.com/hugegene/termos/releases/latest` without following the redirect. Checked on
   2026-11-10: it answers `302` with `location: https://github.com/hugegene/termos/releases/tag/v0.2.0`, in
   0.04 s. That's one request, with no API key and no API rate limit.
2. It compares that tag with its own version. If the tag is newer, the status bar shows the note until the
   next attach. The check runs in the background and is skipped on any error, so being offline costs
   nothing.
3. `tos update` runs `curl -fsSL …/releases/latest/download/install.sh | sh` (or `wget`), passing on
   `TOS_INSTALL_DIR` and `--version` as `TOS_VERSION`. The installer stays the one place that knows how to
   install, which keeps HTTP and TLS code out of `tos`.
4. `install.sh` unpacks to `versions/0.2.2/`, test-runs it, and points the symlink at it in one step (make a
   temporary symlink, then rename it over the old one).
5. It removes versions beyond the newest two. A session still running 0.2.1 keeps working, since its file is
   kept. On Linux and macOS a running program also survives its file being deleted, but its file path
   (`client.rs:41` uses `current_exe` to start servers) is better kept.

A running 0.2.1 server keeps running after the update, as it does today. If the protocol changed, attaching
says to run `tos kill`, and attaching again restores the session under 0.2.2.

## Options considered

- **Background self-update (Claude Code's way):** the smoothest, but it changes the binary under live
  sessions. Not yet.
- **Only a notice, no command:** simpler, but the user still has to find the one-liner. Not chosen.
- **`tos update` in Rust with an HTTP library (`ureq` and rustls):** it needs no `curl`, but adds
  dependencies, TLS code and a second copy of the install logic. Not chosen.
- **Package managers (Homebrew, apt):** they handle updates themselves. They're later items in the
  [distribution plan](2026-11-09-distribution.md) and don't help one-liner users.

## Steps

- [ ] **`install.sh`:** use the versions folder and the symlink, keep the newest two, and turn a plain-file
  `~/.local/bin/tos` (0.2.0 and 0.2.1 installs) into the symlink. Test the upgrade from 0.2.1 locally and in
  a container.
- [ ] **`tos update [--version X]`:** clap definition, `after_long_help` examples, a docs page (regenerated),
  and the concept in `docs/cli.md`. It refuses, explaining why, when `tos` wasn't installed by the installer
  (no versions folder, for example a `cargo build`), so a dev build is never replaced.
- [ ] **The update notice:** a background check on attach, at most daily, with the state file, the status bar
  note and `TOS_NO_UPDATE_CHECK`. Unit-test the version comparison, and use an e2e test against a local
  server through a test-only variable for the base URL.
- [ ] **Release** a version with all three, then upgrade to it from 0.2.1 with `tos update` and record a
  finding: what the upgrade from a plain file did, rollback with `--version`, and the notice appearing.
- [ ] **Distribution plan:** replace its "Later: `tos update`" item with a link here.

## Open questions

- **How many versions to keep:** two is an **estimate**. Each one is about 9–10 MB, so space isn't the limit
  (Claude Code keeps several, at about 340 MB each).
- **Where the notice shows:** the status bar is the obvious place. A one-line message when attaching from a
  shell would be another.
