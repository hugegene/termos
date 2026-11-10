# Distribution: prebuilt binaries and a one-line install

## Decision

People install `tb` with one line, without Rust and without sudo:

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
- **What's in each download:** the stripped `tb` binary, `THIRD_PARTY_NOTICES.md` (from `cargo-about`), our
  license text and a short README, packed as `tb-<version>-<target>.tar.gz` plus a `.sha256` file.
- **Optional tools stay separate.** Neovim, poppler (PDF), ffmpeg (video) and gio (trash) are installed by
  the user. `tb` already says which package is missing. Running them as separate programs also keeps their
  GPL/LGPL off `tb` ([video plan](2026-11-07-video-and-office-previews.md)).

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
│ git tag      │─────▶│ │ cargo build --release,   │ gh  │   tb-0.2.0-<target>.tar.gz       │ │
│ git push     │ tag  │ │ strip, notices, tar,     │     │   tb-0.2.0-<target>.tar.gz       │ │
└──────────────┘      │ │ sha256. VM wiped after.  │     │     .sha256                      │ │
                      │ └──────────────────────────┘     └─────────────────┬────────────────┘ │
                      │    served from GitHub's download CDN               │                  │
                      └────────────────────────────────────────────────────┼──────────────────┘
                                                             3 install.sh, │
                                                             4 tarball and ▼ .sha256
                                                        ┌────────────────────────────────────┐
                                                        │ friend's Ubuntu 22.04: curl … | sh │
                                                        │ 5 check sha256, put tb in          │
                                                        │   ~/.local/bin/tb                  │
                                                        └────────────────────────────────────┘
```

1. On the dev box: set `version = "0.2.0"` in `Cargo.toml`, commit, `git tag v0.2.0`, push the tag to
   `termos`.
2. The release workflow starts one throwaway runner per target. Each checks that the tag matches `tb --version`
   (`tb 0.2.0 (extension API 1)`), builds and packs. Then `gh release create` uploads the files to release
   `v0.2.0` in `termos`, using the workflow's built-in `GITHUB_TOKEN`. The runner is deleted after the job. From
   then on, the binaries are stored only as files on that release.
3. The friend runs the one-liner, which downloads `install.sh` from the release. It runs `uname -sm` and gets
   `Linux x86_64`, so it picks `x86_64-unknown-linux-gnu`.
4. It downloads `tb-0.2.0-x86_64-unknown-linux-gnu.tar.gz` and its `.sha256`. It uses GitHub's
   `/releases/latest/download/…` link, which redirects to GitHub's download CDN.
5. It checks the checksum and puts `tb` in `~/.local/bin`, warning if that folder isn't on `PATH`. It prints
   `tb 0.2.0`.

**Where the binary is at each point:**

- **During the build:** on the runner, a throwaway VM that's deleted when the job ends.
- **After the release:** as a file attached to the release in `termos`, not in its git history.
- **On the user's machine:** in `~/.local/bin/tb`.

**Who pays:** nobody. Standard GitHub-hosted runners are free in public repos, macOS included, and release
downloads have no bandwidth limit ([finding](../findings/2026-11-10-github-actions-cost.md)).

**Upgrading with a session running:** the old server keeps running the old binary. If the protocol changed
(`PROTOCOL_VERSION`, now 3), `tb` refuses to attach and says to run `tb kill`. `tb kill` can end servers
started by an older `tb` (`client.rs:229`), and attaching again restores the session. If the protocol didn't
change, nothing is needed until the next restart.

## Options considered

### Hosting

- **Make the source repo public (chosen 2026-11-10):** the simplest option. Source, CI and releases are in one
  repo, and Actions is free. The catch is that the code needs a license (see "Steps").
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
  - Packs the tarballs and `sha256sum` files, then runs `gh release create` in `termos` with `GITHUB_TOKEN`
    (`permissions: contents: write`).
- [ ] **A license for the source and the binary.** Needs a decision, and ideally a lawyer, as the
  [rebuild plan](2026-11-03-rebuild-plan.md) §7 says.
  - *Changed 2026-11-10:* this is now about the source as well. Today it's public with no license file and
    `LicenseRef-Proprietary` in `Cargo.toml`, so people may read it but not copy or reuse it. Picking an open
    license (MIT, Apache-2.0, or both, as Rust projects often do) or a source-available one means adding a
    `LICENSE` file and changing that field.
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

- **Public source?** *Answered 2026-11-10:* yes. The source is public in `termos`, and the plan above was
  updated for it.
- **Product name:** `tb` is a working name, and the repo is now called `termos`. Pick the real name before the
  first release, because the one-liner contains the repo name and the binary is still called `tb`.
- **Who it's for at first:** a few friends (the GitHub URL is enough), or a public launch (worth the domain
  and bucket first)?
- **arm64 Linux runners:** check they're available on the account's plan. If not, cross-compile from x86_64.
  - *2026-11-10:* from memory, not checked: public repos get free arm64 Linux runners (`ubuntu-22.04-arm`).
    The first release run will confirm it.
- **GitHub Actions minutes:** a private repo pays for minutes, and macOS minutes cost more. Check that it fits
  the plan's free allowance.
  - *Answered 2026-11-10* ([finding](../findings/2026-11-10-github-actions-cost.md)): GitHub Free includes $12
    a month. A Linux minute costs $0.006 and a macOS minute $0.062. The `macos` job in `ci.yml` runs on every
    push, so it uses most of the allowance: about 17 pushes a month (**estimate**). Without a card on file,
    runs stop when the allowance runs out, and nothing is charged. Still to decide: add a build cache, run
    `macos` only on `main` or tags, or both.
  - *No longer applies (2026-11-10):* the repo is public, so standard runners cost nothing.
