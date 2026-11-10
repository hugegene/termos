# Distribution: prebuilt binaries and a one-line install

## Decision

People install `tos` with one line, without Rust and without sudo:

*Changed 2026-11-10:* the product is called **termos** and the command **`tos`**; it was `tb`
([rename plan](2026-11-10-rename-to-termos.md)). This plan now uses the new names, except where it describes
release v0.1.1, which was still `tb`. Until `v0.2.0` is released, the one-liner installs `tb 0.1.1`.

```sh
curl -fsSL https://github.com/hugegene/termos/releases/latest/download/install.sh | sh
```

- **Builds:** GitHub Actions builds the release binaries when we push a version tag (`v0.2.0`). Nothing is
  built on the dev box for a release, and nothing builds automatically after edits (it doesn't today either).
- **Hosting (to start):** one **public** repo, [`hugegene/termos`](https://github.com/hugegene/termos), holds
  the source, CI and the releases. Download links are anonymous and served from GitHub's CDN. Later, a bucket
  on our own domain gives a shorter URL, as Claude Code does.
  - *Changed 2026-11-10:* the source is public too. Before, it was a private source repo plus a separate public
    `tb-releases` repo. Public repos run Actions for free, macOS included
    ([finding](../findings/2026-11-10-github-actions-cost.md)). The release workflow uploads with the built-in
    `GITHUB_TOKEN`, so there's no second repo and no token to store. The source was pushed on 2026-11-10.
- **Targets (first release):**
  - `x86_64-unknown-linux-gnu`, built on Ubuntu 22.04
  - `aarch64-unknown-linux-gnu` (arm64), if the arm runner is available to us
  - `aarch64-apple-darwin`, once [macOS support](2026-11-09-macos-support.md) is done. The launch release
    must include it.
  - *Changed 2026-11-10:* no Intel Mac build (`x86_64-apple-darwin`). `install.sh` tells Intel Mac users
    it isn't supported.
- **What's in each download:** the stripped `tos` binary, `THIRD_PARTY_NOTICES.md` (from `cargo-about`), our
  license text and a short README, packed as `tos-<target>.tar.gz` plus a `.sha256` file. Inside is one folder,
  `tos-<version>-<target>/`.
  - *Changed 2026-11-10:* the file name has no version (it was `tb-<version>-<target>.tar.gz`). That way
    `install.sh` can fetch `/releases/latest/download/tos-<target>.tar.gz` without first asking GitHub's API
    which version is the latest. The release's tag carries the version.
- **Optional tools stay separate.** Neovim, poppler (PDF), ffmpeg (video) and gio (trash) are installed by
  the user. `tos` already says which package is missing. Running them as separate programs also keeps their
  GPL/LGPL off `tos` ([video plan](2026-11-07-video-and-office-previews.md)).

### Example: releasing 0.2.0 and a friend installing it

*Changed 2026-11-10:* redrawn so that GitHub shows as the host, with where each file is stored. Redrawn again
the same day for one public repo.

```
 dev box (yours)        GitHub, the host: it stores and serves everything in this box
                      ┌───────────────────────────────────────────────────────────────────────┐
                      │ public repo hugegene/termos: source, CI and releases. All free.       │
                      │ ┌──────────────────────────┐     ┌──────────────────────────────────┐ │
┌──────────────┐      │ │ Actions runner, one per  │ 2   │ Release v0.2.0, files stored:    │ │
│ bump version │  1   │ │ target: a throwaway VM   │────▶│   install.sh                     │ │
│ git tag      │─────▶│ │ cargo build --release,   │ gh  │   tos-<target>.tar.gz            │ │
│ git push     │ tag  │ │ strip, notices, tar,     │     │   tos-<target>.tar.gz.sha256     │ │
└──────────────┘      │ │ sha256. VM wiped after.  │     │                                  │ │
                      │ └──────────────────────────┘     └─────────────────┬────────────────┘ │
                      │    served from GitHub's download CDN               │                  │
                      └────────────────────────────────────────────────────┼──────────────────┘
                                                             3 install.sh, │
                                                             4 tarball and ▼ .sha256
                                                        ┌────────────────────────────────────┐
                                                        │ friend's Ubuntu 22.04: curl … | sh │
                                                        │ 5 check sha256, put tos in         │
                                                        │   ~/.local/bin/tos                 │
                                                        └────────────────────────────────────┘
```

1. On the dev box: set `version = "0.2.0"` in `Cargo.toml`, commit, `git tag v0.2.0`, push the tag to
   `termos`.
2. The release workflow starts one throwaway runner per target. Each checks that the tag matches `tos --version`
   (`tos 0.2.0 (extension API 2)`), builds and packs. Then `gh release create` uploads the files to release
   `v0.2.0` in `termos`, using the workflow's built-in `GITHUB_TOKEN`. The runner is deleted after the job. From
   then on, the binaries are stored only as files on that release.
3. The friend runs the one-liner, which downloads `install.sh` from the release. It runs `uname -sm` and gets
   `Linux x86_64`, so it picks `x86_64-unknown-linux-gnu`.
4. It downloads `tos-x86_64-unknown-linux-gnu.tar.gz` and its `.sha256`. It uses GitHub's
   `/releases/latest/download/…` link, which redirects to GitHub's download CDN.
5. It checks the checksum, unpacks, and runs the new `tos --version`. That catches a glibc that's too old before
   anything is replaced. It then puts `tos` in `~/.local/bin`, warning if that folder isn't on `PATH`, and prints
   `installed tos 0.2.0 (extension API 2)`.

**Where the binary is at each point:**

- **During the build:** on the runner, a throwaway VM that's deleted when the job ends.
- **After the release:** as a file attached to the release in `termos`, not in its git history.
- **On the user's machine:** in `~/.local/bin/tos`.

**Who pays:** nobody. Standard GitHub-hosted runners are free in public repos, macOS included, and release
downloads have no bandwidth limit ([finding](../findings/2026-11-10-github-actions-cost.md)).

**Upgrading with a session running:** the old server keeps running the old binary. If the protocol changed
(`PROTOCOL_VERSION`, now 3), `tos` refuses to attach and says to run `tos kill`. `tos kill` can end servers
started by an older `tos` (`client.rs:229`), and attaching again restores the session. If the protocol didn't
change, nothing is needed until the next restart.

## Options considered

### Hosting

- **Make the source repo public (chosen 2026-11-10):** the simplest option. Source, CI and releases are in one
  repo, and Actions is free. The code is licensed MIT (see "Steps").
- **A public repo that holds only releases (chosen first, replaced 2026-11-10):** free downloads and private
  source, but CI in the private repo costs money and the release workflow needs a token for the second repo.
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
  - *2026-11-10:* with one public repo, `dist`'s same-repo model now fits. Not reconsidered; our own scripts
    are still the choice.

### Linux builds

- **glibc, built on Ubuntu 22.04 (chosen):** today's binary already needs only glibc 2.34
  ([finding](../findings/2026-11-09-release-binary-contents.md)). Building on 22.04 (glibc 2.35) makes sure no
  newer requirement creeps in. That covers Ubuntu 22.04+, Debian 12+ and RHEL 9+ (distro glibc versions from
  memory).
- **Static musl (later, if needed):** would also run on Ubuntu 20.04, Debian 11 and RHEL 8. Not tried yet, and
  musl's memory allocator is slower (**estimate**), so it needs a speed check first.

## Steps

- [x] **A GitHub repo for the source, and push** (2026-11-10): [`hugegene/termos`](https://github.com/hugegene/termos),
  public, on a **personal** account. `main` was pushed with 30 commits, and CI started on the push.
  - `gh` was logged in only as a work account that looks enterprise-managed. Such an account can't create
    public repos and can't use GitHub-hosted runners on its own repos
    ([finding](../findings/2026-11-10-github-actions-cost.md)).
  - Before the push, the history was checked for tokens, keys and work names. None were found. Commits carry
    only the user's Gmail address.
  - Pushing needed the `workflow` scope (`gh auth refresh -s workflow`) because of `.github/workflows/ci.yml`.
    Git uses `gh` as its credential helper in this repo only (`git config --local`).
- [x] **Release profile** (2026-11-10): added `strip = true` to `[profile.release]`. That removes 28,243
  symbol names and 3.1 MB. A local build is now 9,627,704 bytes. Re-measure the CI-built binary in a new
  finding.
  - *Changed 2026-11-09:* dropped `--remap-path-prefix`. The only absolute paths are dependency sources under
    the build machine's home, which is the CI runner's. Our own paths are already relative and make panic
    reports useful ([finding](../findings/2026-11-09-release-binary-contents.md), "Added later").
- [x] **`install.sh`** (2026-11-10), published with each release:
  - *Changed:* it lives at **`scripts/install.sh`**, because `install.sh` in the repo root is the prototype's
    setup script.
  - It's POSIX `sh`, since `curl … | sh` runs `dash` on Ubuntu. Everything is in functions, with `main` called
    on the last line, so a download that's cut short runs nothing.
  - **Detects the target** from `uname -sm`: Linux x86_64 or aarch64, or Darwin arm64. A shell under Rosetta
    reports x86_64, so it also checks `sysctl.proc_translated`. Intel Macs, other architectures and other
    systems stop with a reason.
  - **Downloads** with `curl`, falling back to `wget`, and checks the SHA-256 with `sha256sum` (or
    `shasum -a 256` on macOS).
  - **Runs the new `tos --version` before installing.** If it won't run (for example, glibc older than 2.34), it
    stops with the supported distros and leaves the old `tos` alone.
  - **Installs** to `~/.local/bin` by copying next to the old file and renaming over it, so a running `tos`
    keeps its file. `TOS_INSTALL_DIR`, `TOS_VERSION` and `TOS_DOWNLOAD_URL` (for testing) override the defaults.
  - **Prints** the installed version, a `PATH` line if the folder isn't on `PATH`, and, on an upgrade with a
    `tos` server running, the `tos kill` hint.
  - **Tested locally** against `python3 -m http.server` with a locally packed tarball:
    - a fresh install through `curl … | sh` (`dash`)
    - an upgrade
    - `wget` only
    - a wrong checksum (stops, installs nothing)
    - a 404
    - a `tos` that won't run (stops, keeps the old one)
    - a fake `uname` for riscv64, FreeBSD, an Intel Mac, Rosetta (picks `aarch64-apple-darwin`) and Linux
      aarch64
- [x] **`.github/workflows/release.yml`** (2026-11-10):
  - Runs on `v*` tags. A `build` matrix runs on `ubuntu-22.04` (x86_64), `ubuntu-22.04-arm` and
    `macos-latest`.
  - Each build job runs `cargo build --release --locked -p tos --target <target>`. It installs `cargo-about`
    with `taiki-e/install-action`, runs `cargo about generate about.hbs -o THIRD_PARTY_NOTICES.md` and then
    **`scripts/package.sh`**. That script refuses to pack unless the binary's `tos --version` matches the tag,
    which also proves the binary runs on its runner.
  - A `licenses` job runs `cargo deny check licenses`.
  - The `release` job collects the tarballs (upload and download artifacts) and runs `gh release create` with
    the tarballs, `.sha256` files and `scripts/install.sh`, using `GITHUB_TOKEN` (`contents: write`).
  - `scripts/package.sh` also works by hand. It was tested locally, including that it refuses a version
    mismatch.
- [x] **A license for the source and the binary** (2026-11-10): **MIT**, the user's choice. Added `LICENSE`,
  set `license = "MIT"` in `Cargo.toml` and added a License section to the README. The `alacritty_terminal`
  fork stays Apache-2.0 with its own `LICENSE-APACHE`. `cargo deny check licenses` passes. Each release
  tarball carries `LICENSE`.
  - This step first covered only the binary. It covers the source too since the source went public.
- [x] **The first test release `v0.1.1`** (2026-11-10):
  [published](https://github.com/hugegene/termos/releases/tag/v0.1.1) about 3 minutes after the tag
  ([finding](../findings/2026-11-10-first-release.md)).
  - **Installs:** the one-liner installs on this machine, Ubuntu 22.04 and Debian 12 (Docker). Ubuntu 20.04 is
    refused cleanly. A session started, took input and was saved by `tb kill` on 22.04.
  - **glibc:** both Linux builds need glibc 2.34, as measured before.
  - **macOS:** the binary declares macOS 11.0 as its minimum.
  - **Not yet tried:** the macOS binary by hand on a Mac, and the one-liner on a real arm64 Linux machine.
  - **What getting CI green took:** two tests that assumed Linux tools, a timing test too tight for shared
    runners, and two real bugs. On macOS, shells now start without `/usr/bin/login`, and `~` is recognised by
    its real path ([macOS plan](2026-11-09-macos-support.md)). `tb kill` now waits until the session has ended,
    and a renamed session removes its socket.
- [x] **README** (2026-11-10): the tos section has an Install part with the one-liner, then a Development part
  with `cargo build --release` and how to release.
- [ ] **Later:**
  - `tos update` (download, check, replace the binary atomically)
  - our own domain and bucket
  - a Homebrew tap
  - a musl build
  - signed releases (minisign or cosign) on top of checksums

## Open questions

- **Public source?** *Answered 2026-11-10:* yes. The source is public in `termos`, and the plan above was
  updated for it.
- **Product name:** `tb` is a working name, and the repo is now called `termos`. Pick the real name before the
  first release, because the one-liner contains the repo name and the binary is still called `tb`.
  - *Answered 2026-11-10:* the product is **termos** and the command **`tos`**
    ([rename plan](2026-11-10-rename-to-termos.md)).
- **Who it's for at first:** a few friends (the GitHub URL is enough), or a public launch (worth the domain
  and bucket first)?
- **arm64 Linux runners:** check they're available on the account's plan. If not, cross-compile from x86_64.
  - *Answered 2026-11-10:* yes. `ubuntu-22.04-arm` built v0.1.1 in 1 min 37 s
    ([finding](../findings/2026-11-10-first-release.md)).
- **GitHub Actions minutes:** a private repo pays for minutes, and macOS minutes cost more. Check that it fits
  the plan's free allowance.
  - *Answered 2026-11-10* ([finding](../findings/2026-11-10-github-actions-cost.md)): GitHub Free includes $12
    a month. A Linux minute costs $0.006 and a macOS minute $0.062. The `macos` job in `ci.yml` runs on every
    push, so it uses most of the allowance: about 17 pushes a month (**estimate**). Without a card on file,
    runs stop when the allowance runs out, and nothing is charged. Still to decide: add a build cache, run
    `macos` only on `main` or tags, or both.
  - *No longer applies (2026-11-10):* the repo is public, so standard runners cost nothing.
