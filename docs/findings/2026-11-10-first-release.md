# The first release, v0.1.1: what CI built, and the one-liner on clean systems

## Question

Does the release pipeline ([distribution plan](../plans/2026-11-09-distribution.md)) give working binaries
for all three targets? It covers the tag, `release.yml`, `scripts/package.sh` and `scripts/install.sh`. We also
needed to know how big the binaries are, which glibc and macOS versions they need, and whether
`curl … | sh` works on a clean system. Three things depend on this: the plan's "first test release" step, the
supported systems that `install.sh` and the README name, and the plan's open question about arm64 Linux
runners. The plan also asked for the CI-built, stripped binary to be measured again.

## Method

On 2026-11-10. Tag `v0.1.1` on commit `20183f7`, built by release run `42769548587`. The local machine is
Ubuntu 24.04.5 LTS, x86_64, with Docker for the clean systems.

```sh
# The run and the release
gh run view 42769548587 -R hugegene/termos --json jobs --jq '.jobs[] | "\(.name): \(.conclusion) \(.startedAt) \(.completedAt)"'
gh release view v0.1.1 -R hugegene/termos --json assets --jq '.assets[] | "\(.name) \(.size)"'

# The files, unpacked in the scratchpad
gh release download v0.1.1 -R hugegene/termos
sha256sum -c tb-<target>.tar.gz.sha256; tar -xzf tb-<target>.tar.gz -C x/<target>
find x/<target> -type f -printf "%s %P\n"
file x/*/*/tb
nm x/<target>/*/tb                                      # stripped?
readelf -V --wide x/<target>/*/tb | sed -n '/Version needs/,$p' | grep 'Name: GLIBC'   # Flags: none = required
python3 -I macho_minos.py x/aarch64-apple-darwin/*/tb   # reads LC_BUILD_VERSION (minos, sdk)

# The one-liner on this machine (into the scratchpad, not ~/.local/bin)
curl -fsSL https://github.com/hugegene/termos/releases/latest/download/install.sh | TB_INSTALL_DIR=<scratch>/real sh

# The one-liner on clean systems
docker run --rm ubuntu:22.04 sh -c 'apt-get update && apt-get install -y curl ca-certificates &&
  ldd --version | head -1 && curl -fsSL …/install.sh | sh; /root/.local/bin/tb --version'
# the same for debian:12 and ubuntu:20.04
# a session on ubuntu:22.04: tb attach -b smoke; tb ls; tb action -s smoke write-chars --enter 'echo …';
#   tb -s smoke dump-screen; tb kill smoke; tb ls
```

`macho_minos.py` walks the Mach-O load commands and prints `LC_BUILD_VERSION` (0x32): the platform, `minos`
and the SDK.

## Results

### The release run

```
licenses: success                     06:35:40 → 06:36:14   (34 s)
build aarch64-unknown-linux-gnu       06:35:43 → 06:37:20   (1 min 37 s, ubuntu-22.04-arm)
build x86_64-unknown-linux-gnu        06:35:41 → 06:37:45   (2 min 4 s, ubuntu-22.04)
build aarch64-apple-darwin            06:35:42 → 06:38:35   (2 min 53 s, macos-latest)
release: success                      06:38:38 → 06:38:47   (9 s)
```

From tag to published release took 3 min 7 s. The repo is public, so it cost nothing.

### The files

```
install.sh                                   4418
tb-aarch64-apple-darwin.tar.gz            3986830   (+ .sha256)
tb-aarch64-unknown-linux-gnu.tar.gz       4155607   (+ .sha256)
tb-x86_64-unknown-linux-gnu.tar.gz        4304614   (+ .sha256)
```

All three `.sha256` files check out. Each tarball holds one folder, `tb-0.1.1-<target>/`:

| File | x86_64 Linux | arm64 Linux | arm64 macOS |
|---|---|---|---|
| `tb` | 9,645,672 | 8,787,464 | 8,134,944 |
| `THIRD_PARTY_NOTICES.md` | 179,555 | 179,555 | 179,555 |
| `LICENSE` | 1,065 | 1,065 | 1,065 |
| `README.md` | 823 | 823 | 823 |

```
tb (macOS):        Mach-O 64-bit arm64 executable, flags:<NOUNDEFS|DYLDLINK|TWOLEVEL|PIE|HAS_TLV_DESCRIPTORS>
tb (arm64 Linux):  ELF 64-bit LSB pie executable, ARM aarch64, dynamically linked, interpreter /lib/ld-linux-aarch64.so.1
tb (x86_64 Linux): ELF 64-bit LSB pie executable, x86-64, dynamically linked, interpreter /lib64/ld-linux-x86-64.so.2
nm, both Linux binaries: no symbols
THIRD_PARTY_NOTICES.md: 108 headings at levels 2 and 3
```

### What they need

glibc versions in "Version needs", with each one's flags:

```
x86_64:  2.2.5 … 2.17 none; 2.18 WEAK; 2.25 WEAK; 2.27 none; 2.28 WEAK; 2.29 WEAK; 2.30 WEAK;
         2.32 none; 2.33 none; 2.34 none
aarch64: 2.17 none; 2.18 none; 2.25 none; 2.27 none; 2.28 none; 2.29 none; 2.30 none;
         2.32 none; 2.33 none; 2.34 none
```

The weak GLIBC_2.39 references in the local 24.04 build (`pidfd_spawnp`, `pidfd_getpid`) are absent.

```
macOS: platform 1 (macOS), minos 11.0.0, sdk 26.5.0
```

### Installing

| Where | glibc | Result |
|---|---|---|
| This machine, Ubuntu 24.04.5 | 2.39 | `installed tb 0.1.1 (extension API 1)`, plus the PATH hint |
| `ubuntu:22.04` | 2.35-0ubuntu3.15 | installed; `tb --version` prints `tb 0.1.1 (extension API 1)` |
| `debian:12` | 2.36-9+deb12u14 | installed; `tb --version` prints `tb 0.1.1 (extension API 1)` |
| `ubuntu:20.04` | 2.31-0ubuntu9.17 | `error: the downloaded tb doesn't run on this system. On Linux it needs glibc 2.34 or newer: …`, exit 1, nothing installed |

The session on `ubuntu:22.04`:

```
smoke [1 tabs, 0 attached]
echo hello-from-22.04                      (dump-screen shows the typed line)
smoke (EXITED - attach to restore)         (tb ls after tb kill)
```

The arm64 Linux and macOS binaries were each run once, as `tb --version`, by `scripts/package.sh` on their
build runner. It refuses to pack otherwise. This machine has no arm64 emulation (no `qemu-aarch64` in
`binfmt_misc`).

## Conclusions

- **The pipeline works end to end and costs nothing.** It takes about 3 minutes from tag to release. The free
  `ubuntu-22.04-arm` runner exists for this public repo, which answers the plan's open question.
- **glibc 2.34 is confirmed for both Linux architectures**, matching the
  [earlier finding](2026-11-09-release-binary-contents.md). The supported systems in `install.sh` and the
  README hold: Ubuntu 22.04 and Debian 12 install, and Ubuntu 20.04 is refused cleanly before anything is
  replaced.
- **macOS:** the binary declares macOS 11.0 as its minimum. The default trash command still needs macOS 15+,
  which is an **estimate** from the [macOS plan](../plans/2026-11-09-macos-support.md).
- **Stripping worked.** The CI-built x86_64 binary is 9.6 MB (the local stripped build is 9,627,704 bytes),
  and the download is 4.3 MB.
- **Still unknown:**
  - **The Mac by hand:** no one has used the macOS binary yet. CI ran all tests on `macos-latest`, and the
    release runner ran `tb --version`.
  - **arm64 Linux install:** the one-liner hasn't run on a real arm64 Linux machine. Only `tb --version` ran
    on the runner.
  - **Upgrades:** going from one release to the next is untested, because there's only one release.
