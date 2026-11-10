# Distribution: prebuilt binaries and a one-line install

## Decision

People install `tb` with one line, without Rust and without sudo:

```sh
curl -fsSL https://<host>/install.sh | sh
```

- **Builds:** GitHub Actions builds the release binaries when we push a version tag (`v0.2.0`). Nothing is
  built on the dev box for a release, and nothing builds automatically after edits (it doesn't today either).
- **Hosting (to start):** the source repo stays **private**. Releases are published to a separate **public
  repo that holds only releases** (working name `tb-releases`). Its download links are anonymous and served from
  a CDN. Later, a bucket on our own domain gives a shorter URL, as Claude Code does.
- **Targets (first release):**
  - `x86_64-unknown-linux-gnu`, built on Ubuntu 22.04
  - `aarch64-unknown-linux-gnu` (arm64), if the arm runner is available to us
  - `aarch64-apple-darwin`, once [macOS support](2026-11-09-macos-support.md) is done. The launch release
    must include it.
  - *Changed 2026-11-10:* no Intel Mac build (`x86_64-apple-darwin`). `install.sh` tells Intel Mac users
    it isn't supported.
- **What's in each download:** the stripped `tb` binary, `THIRD_PARTY_NOTICES.md` (from `cargo-about`), our
  license text and a short README, packed as `tb-<version>-<target>.tar.gz` plus a `.sha256` file.
- **Optional tools stay separate.** Neovim, poppler (PDF), ffmpeg (video) and gio (trash) are installed by
  the user. `tb` already says which package is missing. Running them as separate programs also keeps their
  GPL/LGPL off `tb` ([video plan](2026-11-07-video-and-office-previews.md)).

### Example: releasing 0.2.0 and a friend installing it

```
 dev box               GitHub Actions (private repo)           tb-releases (public)          friend's Ubuntu 22.04
┌──────────────┐ 1 tag ┌───────────────────────────────┐ 2 gh  ┌──────────────────────┐
│ bump version │ v0.2.0│ matrix: x86_64 linux, arm64   │release│ v0.2.0:              │
│ git tag      │──────▶│ linux, arm64 macOS            │create │  install.sh          │
│ git push     │       │ cargo build --release --locked│──────▶│  tb-0.2.0-<target>   │
└──────────────┘       │ strip, notices, tar, sha256   │       │   .tar.gz, .sha256   │
                       └───────────────────────────────┘       └──────────────────────┘
                                                                   ▲  │      ▲  │
                                               3 GET install.sh    │  │ 4    │  │ 5 GET tarball + .sha256
                                                                ┌──┴──▼──────┴──▼───────┐
                                                                │ curl … | sh           │ 6 check sha256,
                                                                │                       │   ~/.local/bin/tb
                                                                └───────────────────────┘
```

1. On the dev box: set `version = "0.2.0"` in `Cargo.toml`, commit, `git tag v0.2.0`, push.
2. The release workflow checks that the tag matches `tb --version` (`tb 0.2.0 (extension API 1)`), builds each
   target and creates release `v0.2.0` in `tb-releases` with a token stored as a repo secret.
3. The friend runs the one-liner. `install.sh` runs `uname -sm` and gets `Linux x86_64`.
4. It picks `x86_64-unknown-linux-gnu` and resolves "latest" through GitHub's
   `/releases/latest/download/…` link.
5. It downloads `tb-0.2.0-x86_64-unknown-linux-gnu.tar.gz` and its `.sha256`.
6. It checks the checksum and puts `tb` in `~/.local/bin`, warning if that folder isn't on `PATH`. It prints
   `tb 0.2.0`.

**Upgrading with a session running:** the old server keeps running the old binary. If the protocol changed
(`PROTOCOL_VERSION`, now 3), `tb` refuses to attach and says to run `tb kill`. `tb kill` can end servers
started by an older `tb` (`client.rs:229`), and attaching again restores the session. If the protocol didn't
change, nothing is needed until the next restart.

## Options considered

### Hosting

- **A public repo that holds only releases (chosen to start):** free, fast and anonymous, with no server to
  run. The source stays private.
- **Make the source repo public:** simplest for GitHub Releases, but the code is proprietary
  (`LicenseRef-Proprietary`).
- **A bucket on our own domain (R2, S3 or GCS) (later):** a short URL like `https://<domain>/install.sh` and
  full control, as Claude Code does. Costs a domain and a little setup. `install.sh` only needs its base URL
  changed.
- **Package managers:** `cargo install` needs the source, so it's out. A Homebrew tap can point at our
  tarballs; add it after macOS works. An apt repo is more work than it's worth this early.

### Tooling

- **Our own `install.sh` and release workflow (chosen):** about 100 lines each. It works with any hosting,
  including the separate releases repo.
- **`dist` (formerly cargo-dist):** generates both, but it's built around publishing to the same repo's GitHub
  Releases. **Estimate:** its upkeep after 2025 is uncertain.

### Linux builds

- **glibc, built on Ubuntu 22.04 (chosen):** today's binary already needs only glibc 2.34
  ([finding](../findings/2026-11-09-release-binary-contents.md)). Building on 22.04 (glibc 2.35) makes sure no
  newer requirement creeps in. That covers Ubuntu 22.04+, Debian 12+ and RHEL 9+ (distro glibc versions from
  memory).
- **Static musl (later, if needed):** would also run on Ubuntu 20.04, Debian 11 and RHEL 8. Not tried yet, and
  musl's memory allocator is slower (**estimate**), so it needs a speed check first.

## Steps

- [ ] **A private GitHub repo for the source, and push.** Today there's no remote, so this machine holds the
  only copy. You run this, since it publishes outside the machine.
- [ ] **Release profile:** add `strip = true` to `[profile.release]`. That removes 28,243 symbol names and
  3.1 MB. Re-measure the CI-built binary in a new finding.
  - *Changed 2026-11-09:* dropped `--remap-path-prefix`. The only absolute paths are dependency sources under
    the build machine's home, which is the CI runner's. Our own paths are already relative and make panic
    reports useful ([finding](../findings/2026-11-09-release-binary-contents.md), "Added later").
- [ ] **`install.sh`** (in this repo, published with each release):
  - Detects OS and architecture, and fails clearly on anything unsupported.
  - Uses `curl`, falling back to `wget`, and checks the download with `sha256sum` (or `shasum -a 256` on
    macOS).
  - Installs to `~/.local/bin` by default. `TB_INSTALL_DIR` and `TB_VERSION` override that.
  - Prints `tb --version` at the end.
  - Test it locally against a folder served over HTTP, with `TB_INSTALL_DIR` in the scratchpad.
- [ ] **`.github/workflows/release.yml`:**
  - Runs on `v*` tags, with a target matrix.
  - Builds with `cargo build --release --locked`.
  - Runs `cargo about generate about.hbs -o THIRD_PARTY_NOTICES.md` and `cargo deny check licenses`.
  - Packs the tarballs and `sha256sum` files, then runs `gh release create` in `tb-releases`.
- [ ] **License text for the binary** (what users may do with it). Needs a decision, and ideally a lawyer, as
  the [rebuild plan](2026-11-03-rebuild-plan.md) §7 says.
- [ ] **The first test release `v0.1.1`:** install it with the one-liner on this machine and on an Ubuntu
  22.04 machine or VM. That confirms the glibc 2.34 reading in practice.
- [ ] **README:** an Install section with the one-liner. The `cargo build --release` instructions move under
  "Development".
- [ ] **Later:**
  - `tb update` (download, check, replace the binary atomically)
  - our own domain and bucket
  - a Homebrew tap
  - a musl build
  - signed releases (minisign or cosign) on top of checksums

## Open questions

- **Product name:** `tb` is a working name. Pick the real name before the URL and the releases repo are
  public, because renaming later breaks the one-liner.
- **Who it's for at first:** a few friends (the releases repo is enough), or a public launch (worth the domain
  and bucket first)?
- **arm64 Linux runners:** check they're available on the account's plan. If not, cross-compile from x86_64.
- **GitHub Actions minutes:** a private repo pays for minutes, and macOS minutes cost more. Check that it fits
  the plan's free allowance.
