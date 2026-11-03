# Finding: dependencies and licenses for the rebuild

**Date:** 2026-11-03 · **Machine:** Ubuntu (Linux 6.8), the dev box · **Used by:** [rebuild plan](../plans/2026-11-03-rebuild-plan.md)

## Question

Before choosing how to rebuild the terminal workspace, we needed answers to these:
- Which libraries would we depend on, what are their licenses, and are they current and widely used?
- What do zellij and tmux depend on?
- Is `alacritty_terminal` usable as a reusable engine, and what does forking it oblige us to do?
- What licenses does today's prototype carry?

## Method

- **Crate metadata:** the crates.io API, `GET https://crates.io/api/v1/crates/<name>` (generic User-Agent).
  - We recorded `max_stable_version`, that version's `license`, `updated_at` and `recent_downloads` (90 days).
- **zellij's dependencies:** `GET /api/v1/crates/<name>/<version>/dependencies`, keeping only
  `kind == "normal"`, for `zellij`, `zellij-server`, `zellij-client` and `zellij-utils`.
- **Repo files:** HTTP status of `raw.githubusercontent.com/<repo>/<branch>/<file>` (200 = exists, 404 = missing).
- **`alacritty_terminal` modules:** module names scraped from `docs.rs/alacritty_terminal/latest`.
- **tmux:** `dpkg -s tmux` and `/usr/share/doc/tmux/copyright` (the first 5 `License:` lines only).
- **Prototype:** `wc -l` over our glue files; `License:` lines in `/usr/share/doc/<pkg>/copyright`.

```python
# core of the crate check (run with python3 -I)
d = json.load(urlopen(Request(f"https://crates.io/api/v1/crates/{name}", headers={"User-Agent": "license-check-script (crate audit)"})))
c = d["crate"]; ver = c["max_stable_version"] or c["max_version"]
v = next(x for x in d["versions"] if x["num"] == ver)
print(c["name"], ver, v["license"], c["updated_at"][:10], c["recent_downloads"])
```

## Results

### Crates (crates.io, 2026-11-03)

| Crate | Version | License | Last updated | Downloads (90 d) |
|---|---|---|---|---|
| alacritty_terminal | 0.26.0 | Apache-2.0 | 2026-04-06 | 1,430,938 |
| vte | 0.15.0 | Apache-2.0 OR MIT | 2025-02-02 | 25,019,431 |
| vt100 | 0.18.1 | MIT | 2026-10-19 | 8,487,381 |
| termwiz | 0.23.3 | MIT | 2025-03-20 | 13,843,255 |
| tui-term | 0.3.4 | MIT | 2026-04-07 | 383,796 |
| portable-pty | 0.9.0 | MIT | 2025-02-11 | 15,557,568 |
| tokio | 1.54.1 | MIT | 2026-10-21 | 286,512,794 |
| ratatui | 0.30.2 | MIT | 2026-06-19 | 24,849,008 |
| crossterm | 0.29.0 | MIT | 2025-04-05 | 55,614,134 |
| unicode-width | 0.2.3 | MIT OR Apache-2.0 | 2026-10-28 | 210,906,258 |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 | 2026-06-01 | 167,210,914 |
| serde | 1.0.229 | MIT OR Apache-2.0 | 2026-07-18 | 399,859,675 |
| postcard | 1.1.3 | MIT OR Apache-2.0 | 2025-07-24 | 36,363,781 |
| rmp-serde | 1.3.1 | MIT | 2025-12-23 | 30,453,519 |
| rustix | 1.1.6 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 2026-10-27 | 335,233,064 |
| base64 | 0.23.1 | MIT OR Apache-2.0 | 2026-08-04 | 471,082,081 |
| clap | 4.6.7 | MIT OR Apache-2.0 | 2026-09-14 | 274,041,784 |
| toml | 1.1.8+spec-1.1.0 | MIT OR Apache-2.0 | 2026-10-09 | 291,844,563 |
| tracing | 0.1.44 | MIT | 2025-12-18 | 250,659,981 |
| anyhow | 1.0.104 | MIT OR Apache-2.0 | 2026-07-18 | 269,788,358 |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | 2026-09-23 | 485,339,089 |
| dirs | 7.0.0 | MIT OR Apache-2.0 | 2026-09-05 | 102,704,432 |
| notify | 8.2.1 | CC0-1.0 | 2026-10-27 | 52,778,629 |
| frizbee | 0.13.0 | MIT | 2026-08-13 | 273,541 |
| fuzzy-matcher | 0.3.7 | MIT | 2020-10-04 | 9,824,484 |
| nucleo | 0.5.0 | **MPL-2.0** | 2024-04-02 | 684,007 |
| nucleo-matcher | 0.3.1 | **MPL-2.0** | 2024-02-20 | 2,182,055 |
| pulldown-cmark | 0.13.6 | MIT | 2026-10-30 | 65,355,919 |
| syntect | 5.3.0 | MIT | 2025-09-27 | 15,778,029 |
| two-face | 0.5.2+bat-0.26.1 | MIT OR Apache-2.0 | 2026-08-07 | 9,566,387 |
| tree-sitter | 0.27.1 | MIT | 2026-10-08 | 21,194,944 |
| htmd | 0.5.6 | Apache-2.0 | 2026-10-15 | 7,366,196 |
| html2text (Rust) | 0.17.4 | MIT | 2026-10-18 | 1,756,506 |
| html2md | 0.2.17 | **GPL-3.0+** | 2026-08-20 | 233,000 |
| image | 0.25.10 | MIT OR Apache-2.0 | 2026-03-10 | 73,492,627 |
| ratatui-image | 11.1.0 | MIT | 2026-10-18 | 658,067 |
| icy_sixel | 0.7.0 | MIT OR Apache-2.0 | 2026-09-06 | 904,696 |
| csv | 1.4.0 | Unlicense/MIT | 2025-10-17 | 61,603,729 |
| tui-textarea | 0.7.0 | MIT | 2024-10-22 | 702,868 |
| axum | 0.8.9 | MIT | 2026-04-14 | 149,578,705 |
| reqwest | 0.13.5 | MIT OR Apache-2.0 | 2026-09-08 | 244,213,666 |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT | 2026-10-14 | 262,478,780 |
| hmac | 0.13.0 | MIT OR Apache-2.0 | 2026-03-29 | 210,296,688 |
| sha2 | 0.11.1 | MIT OR Apache-2.0 | 2026-10-12 | 348,854,576 |
| rusqlite | 0.41.1 | MIT | 2026-10-28 | 54,398,427 |
| insta | 1.49.0 | Apache-2.0 | 2026-10-03 | 36,590,057 |
| cargo-deny | 0.20.2 | MIT OR Apache-2.0 | 2026-07-09 | 2,209,438 |
| cargo-about | 0.9.3 | MIT OR Apache-2.0 | 2026-10-26 | 337,702 |
| zellij | 0.45.1 | MIT | 2026-08-28 | 15,112 |
| wasmi | 2.0.0 | MIT/Apache-2.0 | 2026-09-01 | 5,721,466 |
| prost | 0.14.4 | Apache-2.0 | 2026-06-07 | 162,642,558 |

### zellij 0.45.1: direct ("normal") dependencies

- **zellij (14):** anyhow clap dialoguer humantime interprocess isahc log miette names suggest thiserror
  zellij-client zellij-server zellij-utils
- **zellij-server (43):** ansi_term anyhow arrayvec async-trait base64 byteorder bytes chrono close_fds
  crossbeam daemonix flate2 highway interprocess isahc kasuari kdl lazy_static libc log nix
  notify-debouncer-full num_cpus once_cell png prost regex serde serde_json signal-hook sixel-image
  sixel-tokenizer sysinfo tempfile tokio unicode-width url uuid vte wasmi wasmi_wasi windows-sys zellij-utils
- **zellij-client (38):** anyhow async-trait axum axum-extra axum-server crossterm daemonix dialoguer
  futures futures-util include_dir interprocess isahc lazy_static log nix num_cpus regex rustls
  rustls-native-certs rustls-pemfile rustls-pki-types serde serde_json signal-hook time tokio tokio-rustls
  tokio-tungstenite tokio-util tower-http url urlencoding uuid windows-sys zellij-utils
  - The API lists `crossterm` and `tokio` twice each; most likely they are platform-specific entries.
- **zellij-utils (48):** anyhow backtrace base64 bitflags clap clap_complete colored colorsys crossbeam
  crossterm curl-sys directories dunce humantime include_dir interprocess isahc kdl lazy_static libc log
  log4rs miette names nix notify openssl-sys percent-encoding prost rusqlite serde serde_json sha2
  shellexpand strip-ansi-escapes strum strum_macros suggest tempfile thiserror tokio tokio-stream
  tokio-util unicode-width url uuid winapi windows-sys

### tmux (Ubuntu package)

- **Version:** `3.4-1ubuntu0.2`
- **Depends:** libc6, libevent-core-2.1-7t64, libsystemd0, libtinfo6, libutempter0
- **Copyright `License:` fields** (first 5 only): BSD-2-clause, BSD-2-clause-NetBSD, "BSD-2-clause or ISC",
  BSD-3-clause, ISC
- **Sixel:** `strings $(command -v tmux) | grep -ci sixel` → 9. This is **inconclusive**: it doesn't prove
  this build has sixel enabled.

### Upstream repos (raw.githubusercontent.com)

| File | Status |
|---|---|
| alacritty/alacritty `LICENSE-APACHE` | 200 |
| alacritty/alacritty `alacritty_terminal/LICENSE-APACHE` | 200 |
| alacritty/alacritty `NOTICE` | **404** |
| zellij-org/zellij `LICENSE.md` | 200: "MIT License / Copyright (c) 2020 Zellij contributors" |
| zellij-org/zellij `NOTICE` | **404** |

### `alacritty_terminal` modules (docs.rs, latest)

- **Top level:** `event`, `event_loop`, `grid`, `index`, `selection`, `sync`, `term`, `thread`, `tty`, `vi_mode`
- **`term::`:** `cell`, `color`, `search`, `test`
- **`vte` (default features):** exposes the `Perform` trait.

### Prototype (`~/terminal-browser`)

- **Glue code:** `finder`, `bin/files`, `bin/send`, `preview.py`, `yazi/init.lua`,
  `yazi/plugins/tfinder.yazi/main.lua`, `nvim/init.lua` and `visidata/visidatarc` total **1,137 lines**.
- **Debian package licenses:**

  | Package | License |
  |---|---|
  | visidata | **GPL-3+** |
  | python3-html2text | **GPL-2+ / GPL-3** |
  | chafa | Expat + **LGPL-3.0+** |
  | bat | MIT |
  | w3m | permissive (w3m) |

- **Local toolchain:** `cargo` and `rustc` are **not installed**. zellij 0.44.1 is installed; crates.io's
  latest is 0.45.1.

## Conclusions

- **The planned stack is all permissive:** MIT, Apache-2.0, ISC, CC0 and Unlicense, all compatible with a
  closed commercial product.
- **Excluded:**
  - `html2md` (GPL-3.0+)
  - `nucleo` and `nucleo-matcher` (MPL-2.0)
  - the prototype's GPL/LGPL tools (VisiData, html2text, chafa)
- **`alacritty_terminal` ships the engine we need:** grid, selection, search, vi mode, PTY (`tty`) and event
  loop. Key encoding and output to the user's terminal are not in its module list and are ours to write.
- **Fork obligations:** neither Alacritty nor zellij has a NOTICE file. Forking `alacritty_terminal` only
  requires the Apache-2.0 license text plus marking the files we change.
- **The stack is proven in production:** zellij already uses `tokio`, `serde`, `crossterm`, `clap`,
  `notify`, `rusqlite`, `axum` and `rustls`. zellij builds its own grid on `vte`; tmux implements
  everything in C on top of libevent and terminfo.
- **Watch items:**
  - `frizbee` is young and has low downloads (~274k in 90 days); `fuzzy-matcher` (MIT, last updated 2020) is the fallback.
  - `portable-pty` was last released 2025-02; the fork's `tty` module may replace it.

**Still unknown:**
- licenses of the syntax and theme data files bundled with `syntect` and `two-face`
- the full license list in tmux's copyright file (only the first 5 entries were read)
- whether Ubuntu's tmux build has sixel enabled
