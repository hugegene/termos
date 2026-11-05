# Rebuild plan: our own terminal workspace

**Status:** proposal · **Date:** 2026-11-03 · **Audience:** product and engineering ·
**Findings:** [dependencies and licenses](../findings/2026-11-03-dependency-licenses.md) ·
[iTerm2 image support](../findings/2026-11-03-iterm2-image-support.md)

## Decision

Build our own product in **Rust**. **Fork `alacritty_terminal`** (Apache-2.0) as the terminal-emulation
engine, and **write everything else ourselves**:

- **Session server and client:** a multiplexer with tabs, splits, attach/detach, persistence and SSH access from a phone.
- **File finder:** previews and editing for Markdown, text/code, CSV, HTML and images.
- **Background daemon:** the poller and webhook server.
- **Extensions through the CLI:** other people extend `tb` with programs that call its CLI, which copies
  zellij's commands. There are no plugins. Step 6 ([CLI extension plan](2026-11-05-cli-extensions.md)).

**Neovim** stays as an external editor program. **zellij** remains our daily driver until our build reaches
step 3 of the roadmap.

Why this option:
- We get a mature terminal emulator without writing the hardest part ourselves.
- Every other line of the product is our own code, which is the cleanest IP position of the options below.
- Both licenses allow a closed, commercial product.

---

## 1. Where we are today (prototype)

`~/terminal-browser` is a working two-pane finder made of about 1,100 lines of our own glue around existing
tools:

| Part | Tool | License |
|---|---|---|
| Panes, tabs, sessions | zellij | MIT |
| File list | yazi | MIT |
| Preview pane | our `preview.py` | ours |
| Markdown / HTML preview | glow (+ html2text for HTML) | MIT (html2text: **GPL-3**) |
| Text and code preview | bat | MIT/Apache |
| Image preview | chafa | **LGPL-3** |
| HTML fallback | w3m | permissive |
| Editing | Neovim + render-markdown.nvim | Apache-2.0 / MIT |
| CSV editing | VisiData | **GPL-3** |

The glue was the source of almost every problem we hit:

- **Images:** zellij only passes **sixel** images through. Ghostty and Terminator don't support sixel, so
  images appear as coloured blocks.
- **Key clashes:** zellij owns Ctrl+Q, which quits everything, and Ctrl+S. The editors had to use Alt+s and Alt+q.
- **Startup races:**
  - yazi crashed when zellij started it before the pane had a size.
  - `ya sub` needed a retry loop.
- **Cross-process plumbing:** a fifo, pane IDs and `zellij action focus-pane-id` are all needed just to move
  between panes.
- **Install:** apt packages, a Neovim tarball and a git clone, instead of one binary.
- **No phone layout:** the 35/65 split is unusable at 40–60 columns.
- **Reboot recovery:** the finder tab probably won't come back after a reboot, because its run folder in
  `/run/user` is wiped. This is untested.
- **Licensing:** the GPL and LGPL tools can't be bundled into a closed product.

## 2. Requirements

1. **For other people, not just us.** That means documentation, configuration and support for a wide range of terminals.
2. **A multiplexer:** shell tabs, split panes, **attach/detach**, and sessions that **persist**.
3. **SSH from a phone** into a running session.
4. **A file finder:** preview and edit md, txt/code, csv, html and images, with mouse support.
5. **Poller + webhook server:** existing components, not yet reviewed (see open questions).
6. **IP we can claim**, with clean license compliance.

## 3. Options considered

| Option | What it means | Verdict |
|---|---|---|
| **A. Keep the glue prototype** | zellij + yazi + CLI tools | ❌ All the problems above; only the glue is ours; GPL parts can't ship |
| **B. Our finder inside zellij** | Our app does the panes, zellij does the sessions | ❌ zellij still sits between us and the terminal (no kitty images, key clashes); users must run zellij; tabs and splits aren't ours |
| **C. Fork zellij** | Rebrand and extend zellij (MIT) | ⚠️ Mature on day one and commercially allowed, but the **weakest IP position**: the product is mostly their code. We'd own a large codebase, keep merging upstream, and have to rebrand |
| **D. Everything from scratch** | Including our own terminal grid on top of the `vte` parser, as zellij did | ⚠️ Maximum control, but the slowest path through the hardest area: thousands of lines and years of compatibility fixes |
| **E. Fork `alacritty_terminal`, build the rest** | Our product around a forked emulation engine | ✅ **Chosen** |

**tmux** (ISC, written in C) is not something we can build on, because it's a program rather than a library.
It is our best **reference** for how to handle edge cases.

### Why fork `alacritty_terminal` instead of just depending on it

- **Stable API on our terms.** Upstream changes its API between releases, because it's built for Alacritty's own needs.
- **Hooks Alacritty will never add.** For example, kitty images use an escape-sequence type that its parser
  drops silently, and we need to intercept those. We also need to intercept iTerm2's image sequences
  (OSC 1337).
- **Fixes without waiting** for upstream.

Rules for the fork:
- It starts as an **unmodified copy** of 0.26.0.
- We keep our changes **small and clearly marked**.
- We merge upstream releases on a schedule.
- Generic fixes go back upstream, so we don't carry them forever.

## 4. Terminal emulation: how the three approaches compare

Each pane needs four layers:
1. **Parse** the bytes a program prints.
2. **Keep screen state:** the grid, scrollback and modes, plus re-wrapping lines on resize.
3. **Talk back:** answer queries, and encode keys and mouse events per pane.
4. **Output:** draw every pane onto the user's real terminal.

| Layer | zellij (`vte` + own grid) | `alacritty_terminal` | tmux |
|---|---|---|---|
| Parse | `vte` crate | `vte` (built in) | Own C parser, same Paul Williams design |
| Screen state, scrollback, re-wrap | Written itself | **Included** | Written itself, very compact |
| Selection, search, vi-style copy mode | Written itself | **Included** | Written itself |
| Answering queries | Written itself | **Included** (as events) | Written itself |
| Key/mouse encoding per pane | Written itself | **Not included**: we write it | Written itself |
| Drawing to the real terminal | Written itself (diff-based redraw) | **Not included**: we write it | Written itself (replays operations via terminfo) |
| Images | Sixel | **None**: we add them | Sixel (3.4+, build option) |
| Spawning PTYs | `nix` | Included (`tty` module) | Written itself |
| Reusable as a library | Only `vte` | **Yes** | No |
| Maturity | Youngest grid | Used daily in Alacritty; Zed's terminal is built on it | Since 2007, the compatibility benchmark |

**What we write on top of the fork:**
- per-pane key and mouse encoding
- output: copy each pane's cells into a `ratatui` buffer and let `ratatui` send only what changed
- image handling (kitty, iTerm2, sixel, colour blocks)

**Images:** we support the **kitty**, **iTerm2** and **sixel** protocols, with coloured blocks as the
fallback.
- **Preferred order** when a terminal supports several: kitty, then iTerm2, then sixel. iTerm2 sends a
  compressed full-colour image, while sixel is palette-based.
- **Detection (changed 2026-11-04):** the client detects what its terminal supports, using a kitty graphics
  query, the terminal's device attributes and `TERM_PROGRAM`, and tells the server. The server then encodes
  images for each client with our own small encoders, so we choose the order. See
  [build steps 4–5](2026-11-04-build-steps-4-5.md).

**Output cost over mobile SSH.**
- **tmux** sends cheap "scroll this region" operations.
- **Frame diffing** (our approach) re-sends a whole pane when it scrolls.
- **Mitigation:**
  - Cap the frame rate and batch updates.
  - Add a scroll optimisation if phone use shows it's needed.

## 5. Architecture

```
phone SSH app ──ssh/mosh──┐
desktop ──────────────────┤   client: thin; sends keys/mouse/resize, receives frames
                          ▼   Unix socket (local only)
   session server
     ├─ tab: finder   (file list │ preview │ Neovim pane)   ← logic lives in the finder-core library
     ├─ tab: shell │ shell        ← split
     └─ tab: shell
     every pane = PTY + terminal emulator (our alacritty_terminal fork)
             ▲   local socket (events, e.g. "new file arrived"; pane text for the phone)
   daemon: poller + webhook listener (public, signature-checked)
         + phone API (private: tailnet-only + paired devices) ◄──HTTPS/WebSocket── phone PWA
```

The phone API and PWA come from the [phone interface plan](2026-11-03-phone-interface.md).

- **Client:** puts the terminal in raw mode, parses input (`crossterm`), forwards it, draws the frames it
  receives, and handles detach.
- **Session server:** owns the PTYs, panes, the per-tab split tree, rendering for each client, state
  persistence and the CLI.
  - It is kept small and stable, because a crash kills every shell.
- **Finder:** a built-in app tab. It renders previews itself, opens Neovim in an embedded pane, and includes a
  CSV grid editor.
- **Daemon:** a **separate process**, so a webhook bug can't take sessions down. It pushes events to the session server.
- **CLI (from step 1):** `attach`, `ls`, `action new-tab|split|focus|close`, `list-panes`, `dump-screen`.
  - It is needed for scripting, for the daemon, and for automated testing. The prototype was tested
    through `zellij action`.
  - **From step 6 it is the extension API:**
    - every zellij command that maps onto a `tb` feature, with zellij's names and flags
    - stable tab and pane ids
    - JSON output
    - an event stream (`tb subscribe`)
    - a promise to only ever add to it
  - See the [CLI extension plan](2026-11-05-cli-extensions.md).

## 6. Library stack

Licenses and versions were checked on crates.io on 2026-11-03. Every crate below allows a closed,
commercial product with a third-party notices file.

### Session server and client

| Job | Crate | Version | License |
|---|---|---|---|
| Terminal emulation | **`alacritty_terminal` (our fork)** | 0.26.0 | Apache-2.0 |
| Async runtime, sockets, processes | `tokio` | 1.54.1 | MIT |
| Spawning PTYs (optional, since the fork has `tty`) | `portable-pty` | 0.9.0 | MIT |
| UI drawing and diffing | `ratatui` | 0.30.2 | MIT |
| Outer terminal I/O | `crossterm` | 0.29.0 | MIT |
| Character widths, emoji | `unicode-width`, `unicode-segmentation` | 0.2.3, 1.13.3 | MIT/Apache |
| Client↔server messages | `serde` + `postcard` | 1.0.229, 1.1.3 | MIT/Apache |
| Socket permission checks, signals | `rustix` | 1.1.6 | Apache/MIT |
| OSC 52 clipboard | `base64` | 0.23.1 | MIT/Apache |
| CLI | `clap` | 4.6.7 | MIT/Apache |
| Config and layouts | `toml` | 1.1.8 | MIT/Apache |
| Logging, errors, folders | `tracing`, `anyhow`, `thiserror`, `dirs` | 0.1.44, 1.0.104, 2.0.21, 7.0.0 | MIT/Apache |

### Finder and previews

These replace yazi, glow, bat, chafa, html2text, w3m and VisiData.

| Job | Crate | Version | License |
|---|---|---|---|
| Refresh on file changes | `notify` | 8.2.1 | CC0 |
| Fuzzy file search | `frizbee` | 0.13.0 | MIT |
| Markdown rendering | `pulldown-cmark` | 0.13.6 | MIT |
| Syntax highlighting | `syntect` + `two-face` | 5.3.0, 0.5.2 | MIT, MIT/Apache |
| HTML → Markdown | `htmd` | 0.5.6 | Apache-2.0 |
| Image decoding | `image` | 0.25.10 | MIT/Apache |
| Image display (kitty, iTerm2, sixel, blocks; auto-detected) | ~~`ratatui-image`, `icy_sixel`~~ our own encoders (changed 2026-11-04), PNG via `image` | n/a | ours |
| CSV read/write and grid editor | `csv` | 1.4.0 | MIT/Unlicense |
| PDF text and pages | poppler's `pdfinfo`, `pdftotext`, `pdftoppm`, run as separate programs ([PDF plan](2026-11-05-pdf-preview.md)) | 24.02 tested | GPL (not linked or bundled) |

### Daemon (poller + webhook)

| Job | Crate | Version | License |
|---|---|---|---|
| Webhook HTTP server | `axum` | 0.8.9 | MIT |
| Poller HTTP client | `reqwest` | 0.13.5 | MIT/Apache |
| TLS | `rustls` | 0.23.46 | Apache/ISC/MIT |
| Webhook signature checks | `hmac` + `sha2` | 0.13.0, 0.11.1 | MIT/Apache |
| State and de-duplication | `rusqlite` (bundled SQLite) | 0.41.1 | MIT (SQLite: public domain) |

### Development only (not shipped)

| Job | Tool | License |
|---|---|---|
| Snapshot tests of rendered screens | `insta` | Apache-2.0 |
| CI license gate | `cargo-deny` | MIT/Apache |
| Generating the third-party notices file | `cargo-about` | MIT/Apache |

### External programs

- **Neovim** (Apache-2.0, plus Vim-licensed code) with **render-markdown.nvim** (MIT) for live Markdown
  editing. We either require it or bundle it with its licenses, and fall back to the user's `$EDITOR`.
- **poppler-utils** (GPL) for PDF previews. It's optional, and we don't bundle it: the user installs it, and
  without it a PDF's preview says which package to install.
  - Running it as a separate program keeps its licence off `tb`.
  - It also means killing the program cancels a slow page, and a crash in it can't take the server down.

### Avoid

| Crate / tool | License | Reason |
|---|---|---|
| `html2md` | GPL-3.0+ | Copyleft; use `htmd` |
| `nucleo`, `nucleo-matcher` | MPL-2.0 | Changes to its files must be published; use `frizbee` |
| html2text (Python), VisiData | GPL-3 | Copyleft |
| chafa | LGPL-3 | Can't be bundled cleanly |

**Two caveats:**
- **Syntax and theme files:** the definitions bundled with `syntect` and `two-face` are data files with their
  own licenses, and `cargo-about` does not see them. Review them by hand.
- **Indirect dependencies:** our ~25 crates pull in a few hundred more (mostly MIT/Apache, some
  BSD/ISC/Zlib/Unicode). `cargo-deny` checks them on every build.

### For reference: what zellij and tmux use

- **zellij 0.45.1** (MIT, Rust): about 100 direct dependencies across four crates. It shares `tokio`,
  `serde`, `crossterm`, `clap`, `notify`, `rusqlite`, `axum` and `rustls` with our list. It differs in a
  few places:
  - `vte` plus its own grid, where we use `alacritty_terminal`
  - `nix` for PTYs
  - `wasmi` for WebAssembly plugins
  - `prost` (protobuf) for messages
  - `kdl` for config
- **tmux 3.4** (ISC, C): links libevent and terminfo (ncurses), and on Ubuntu also libutempter and libsystemd.
  It wrote everything else itself.

## 7. Licensing and IP

| | zellij (MIT) | `alacritty_terminal` (Apache-2.0) |
|---|---|---|
| Commercial use, closed source | ✅ | ✅ |
| Keep copyright and license text | Required | Required |
| Mark files we change | No | **Required** ("modified by…") |
| Patents | Not mentioned | Contributors grant us patent rights; we lose them if we sue claiming the code infringes our patents |
| Use their name or logo | Trademark law applies | Explicitly not granted |
| Our own license on our changes | ✅ | ✅ |

Neither repo has a NOTICE file (checked on 2026-11-03), so there are no extra attribution texts to carry.

**Compliance checklist:**
- [ ] The fork lives in its own crate or folder, with Alacritty's `LICENSE-APACHE` and full upstream git history.
- [ ] Every changed file in the fork carries a "Modified by … (date): what" note.
- [ ] `cargo-about` generates `THIRD_PARTY_NOTICES` for every release; we ship it, and Neovim's licenses if we bundle it.
- [ ] `cargo-deny` in CI allows MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, CC0-1.0 and Unlicense. It
      blocks GPL, AGPL and LGPL, and flags MPL for review.
- [ ] The product name and logo don't use "Alacritty", "zellij" or "tmux". "Based on…" is fine.
- [ ] zellij and tmux are references for **behaviour** only; we don't copy their code. If we ever copy a
      snippet, its license notice comes with it.

**IP notes:**
- **What we own:** our own code and our changes to the fork. The upstream code stays its authors' copyright,
  licensed to us. That's normal and accepted in investor and acquisition reviews when the notices are in order.
- **AI-assisted code:** Anthropic's terms assign the output to us, but US copyright requires human
  authorship, so code that is mostly generated may be only weakly protected. Our stronger protections are:
  - keeping the source private (trade secret)
  - our own design decisions and edits
  - contracts
- This summarises how the licenses read; it is **not legal advice**. Have a lawyer review the notices and
  IP position before fundraising or a sale.

## 8. Scope compared with zellij

Because this is for other people, some zellij features become expected.

| Feature | Plan |
|---|---|
| Tabs, splits, attach/detach, persistence | ✅ Core (steps 1–4) |
| Scriptable CLI (`action`, `list-panes`, `dump-screen`) | ✅ From step 1; all of zellij's CLI, as the extension API, in step 6 |
| Scrollback search, copy mode, clipboard | ✅ Step 3 |
| Floating panes (pop-up shell) | ✅ Step 8; useful on phones |
| Layout files, themes, config | ✅ Small version for launch; keys that run commands come earlier, in step 6 |
| Key modes with on-screen hints | ✅ Simple version for launch |
| Stacked panes, auto-arrange layouts | Maybe |
| Plugin system (WebAssembly) | ❌ Extensions use the CLI instead (step 6) |
| Built-in web client | ❌ SSH covers phones |
| Several people in one session | ❌ Unless customers ask |

**What no rebuild can replace:**
- **Maturity:** zellij has years of fixes contributed by many users. We only get that by using our build every day.
- **Running processes after a reboot:** no multiplexer can restore them. We restore the layout, folders and app state.
- **The user's terminal:** if a phone terminal can't show images, no server can fix that.

**Where we beat zellij:**
- real images in Ghostty (kitty protocol)
- keys we fully control
- the finder built in
- a layout per device
- live events from the daemon
- code we own

## 9. Sessions, persistence and phone access

**Persistence has two levels:**
1. **Surviving a disconnect** (laptop closed, phone connection dropped). The server keeps running, so this comes
   free with attach/detach.
2. **Surviving a reboot.** We save the layout, each pane's working folder and the finder state (folder,
   selection, open files, scroll position, view mode), then reopen shells in those folders. Neovim's undo
   history and swap files protect edits.

**Phone over SSH** (`ssh host`, then `<product> attach`):
- **Narrow screens** (below about 80 columns) automatically show one pane at a time, with a tappable tab bar.
- **Several devices at once:** each client gets its own layout for our widgets. A shell or Neovim pane has
  one size, which follows whichever device typed last.
- **Keys:** every action needs a plain key or a tap, because Alt and Ctrl are awkward on phone keyboards.
  Termux and Blink can send taps as mouse clicks.
- **Flaky networks:** Mosh, or simply reconnect and re-attach. Mosh doesn't pass images through.
- **Images:** Blink (plain SSH only, not Mosh) and ConnectBot can show iTerm2 images. Other phone terminals
  get the coloured-block fallback ([phone research](../findings/2026-11-03-phone-interface-research.md) §1).
- **Security:**
  - The session server listens only on a Unix socket.
  - The public webhook listener is kept separate from the private phone API.
  - The phone API binds only to Tailscale and accepts only paired devices (see the
    [phone interface plan](2026-11-03-phone-interface.md)).
  - Use Tailscale or WireGuard for SSH from the phone instead of exposing SSH to the internet.

**Phone app (PWA):** browsing, previews, editing and push notifications come from a phone-native PWA over our
own API, not from the terminal. See the [phone interface plan](2026-11-03-phone-interface.md).

## 10. Hard parts and risks

| Risk | Mitigation |
|---|---|
| Long tail of terminal compatibility (htop, vim, ssh, Claude Code, emoji widths, mouse modes) | Mature emulation in the fork; daily use of our own build; a test matrix of real programs |
| Per-pane key encoding (application cursor keys, kitty keyboard protocol, bracketed paste) | Follow tmux's behaviour; snapshot tests |
| Output cost on slow links | Frame-rate cap, batching, and a later scroll optimisation |
| Multi-client sizes | Size per client for our widgets; shared PTY panes follow the active client |
| Images through the multiplexer | Intercept kitty/iTerm2/sixel sequences in the fork; block fallback |
| Server crash kills every shell | Small session server; daemon in a separate process; crash logs |
| Upgrading the server while sessions run | The old server keeps serving its sessions until they close; versioned client↔server protocol |
| Fork drift from upstream | Minimal, marked changes; scheduled merges; send generic fixes upstream |
| License slip-ups in indirect dependencies | `cargo-deny` in CI from the first commit |

## 11. Roadmap

Sizes are rough estimates.

| Step | Scope | Outcome |
|---|---|---|
| **0. Setup** | Install Rust (rustup), create the repo, fork `alacritty_terminal` 0.26.0 unmodified, add `cargo-deny`, `cargo-about` and CI | Clean licensing from commit one |
| **1. Core** | Server, client, attach/detach, CLI (with `list-panes` and `dump-screen` for tests); one tab running the finder (file list, previews, Neovim pane) | Matches today's prototype |
| **2. Multiplexer** | Shell tabs and splits: new, split, focus, close, zoom, resize, mouse | Usable multiplexer (**~3–5k lines** total) |
| **3. Daily driver** | Scrollback, copy mode, search, OSC 52 clipboard, mouse selection | **Switch our daily use from zellij** |
| **4. Persistence** | Reboot recovery: layout, folders, names, finder state; programs that were running come back as commands waiting at the prompt | Sessions survive reboots, crashes and Ctrl+Q |
| **5. Finder extras** | CSV grid editor; kitty, iTerm2 and sixel images | Full replacement for the prototype |
| **6. CLI and extensions** | Every zellij command `tb` has the features for, with zellij's names and flags; stable tab and pane ids; JSON output; events (`tb subscribe`); keys that run commands (zellij's keybind syntax); status-bar text from extensions; a stable, documented API ([CLI extension plan](2026-11-05-cli-extensions.md)) | Other people extend `tb` from any language; zellij scripts such as `prbot` port by renaming (**~3–5k lines**) |
| **7. Phone** | Everything for phones: the one-pane layout below ~80 columns, per-client layouts, plain keys and taps for every action; then the PWA ([phone plan](2026-11-03-phone-interface.md) P1–P4) | SSH-from-phone ready (**~10–15k lines** total before the PWA) |
| **8. Floating panes** | Floating (pop-up) panes; maybe stacked and pinned panes and swap layouts, zellij's other gaps; their 12 CLI actions | No missing zellij pane features |
| **Parallel track** | Daemon: poller, webhook, events into the finder; drives sessions through the step 6 CLI | Independent process; can start anytime |
| **9. Launch** | Config, themes, layouts, key hints, docs, packaging, testing across terminals (Ghostty, Terminator, WezTerm, kitty, iTerm2 if macOS is supported, Termux, Blink) | Ready for other users (roughly **2×** the personal-use effort) |

The prototype in `~/terminal-browser` stays in use until step 3. Its preview logic (file-type detection, the
CSV table layout, rendering HTML via Markdown) and its notepad-style Neovim config carry over as reference.

## 12. Open questions

1. **Product name and repo location.** The name must avoid "zellij", "Alacritty" and "tmux".
2. **Poller and webhook server:** where is the code, what does it poll, and what events should reach the finder?
3. **Neovim:** bundle it or require it?
4. **Platforms:** Linux only at launch, or macOS too?
5. **Legal review:** when, relative to fundraising or customers?
6. **Rust toolchain:** not installed on the dev machine yet (`curl --proto '=https' -sSf https://sh.rustup.rs | sh`).

## Change log

- **2026-11-03:** first version.
- **2026-11-03:** phone interface decided ([phone plan](2026-11-03-phone-interface.md)). Three changes:
  - **finder-core:** the finder's logic becomes a library shared by the TUI and the phone API, from step 1.
  - **Phone API:** the daemon gains a private phone API (tailnet-only, paired devices). This replaces
    "SSH is the only way in"; the public webhook listener stays separate.
  - **Narrow layout:** the single-pane layout is now backed by measurements. 40–50 columns was unusable
    two-pane and 100 columns was fine, so it kicks in below ~80 columns (exact threshold to confirm).
- **2026-11-03:** iTerm2 image support added, next to kitty and sixel
  ([finding](../findings/2026-11-03-iterm2-image-support.md)). No new crate is needed, because
  `ratatui-image` already supports it. Updated:
  - the fork reasons (§3) and an images note (§4)
  - the image row (§6) and the phone images note (§9)
  - the images risk (§10), and steps 5 and 6 (§11)
- **2026-11-04:** roadmap steps 0–3 built in this repo as `tb`, a working name. Details, departures from this
  plan and known gaps are in the [build plan](2026-11-03-build-steps-0-3.md). Two departures matter here:
  - The session server uses threads, not tokio.
  - The fork records its upstream commit instead of importing Alacritty's git history.
- **2026-11-04:** roadmap reordered at the user's request:
  - Step 4 keeps reboot recovery only.
  - Everything for phones moved to a new step 6: the narrow one-pane layout, per-client layouts, phone keys
    and the PWA.
  - Floating panes moved out of step 5 into a new step 7.
  - Launch is now step 8.
- **2026-11-04:** step 5 images will use **our own small encoders** (kitty, iTerm2, sixel) instead of
  `ratatui-image`, because:
  - `ratatui-image`'s default features link `libchafa` (LGPL).
  - Its detection runs inside the app's process, but our server draws for several clients, each with its own
    terminal.
  - Its font-size constructor for that case is deprecated.

  See [build steps 4–5](2026-11-04-build-steps-4-5.md).
- **2026-11-05:** new **step 6, CLI and extensions.** The user's aim is that other people can extend `tb`
  through its CLI.
  - **What it covers:** the CLI copies zellij's commands (72 actions, measured in the
    [actions finding](../findings/2026-11-05-zellij-actions.md)), adds stable ids, JSON output and an event
    stream, and promises not to break.
  - **Plugins:** the "Plugin system" row now points to the CLI.
  - **Order:** it comes before the phone, because it's smaller and it unblocks scripts like `prbot`.
  - **Renumbered:** Phone is now step 7, floating panes step 8 (with their 12 CLI actions), and launch step 9.
  - Plan: [CLI extension plan](2026-11-05-cli-extensions.md).
- **2026-11-05:** step 6 also gets **keys that run commands** and **status-bar text from extensions**.
  - Keys use zellij's keybind syntax in `~/.config/tb/config.kdl`, the file the launch config will grow.
  - The step's estimate is now ~3–5k lines.
- **2026-11-05:** **step 6 built.**
  - The CLI is the extension API, with its reference in [docs/cli.md](../cli.md).
  - `prbot` runs on `tb` with 7 lines changed ([finding](../findings/2026-11-05-prbot-on-tb.md)).
  - The protocol version is now 3.
  - Details are in the [CLI extension plan](2026-11-05-cli-extensions.md).
- **2026-11-05:** the finder **previews PDFs** with poppler's command-line tools, run as separate programs.
  - **Text first;** `t` switches to the pages, at fit width with zoom.
  - **poppler-utils** is a new optional external program (§6).
  - Details are in the [PDF preview plan](2026-11-05-pdf-preview.md).
- **2026-11-05:** the finder is now **two panes**, its file list and its preview, as in the zellij version.
  - Its border drags, and resize mode and fullscreen work on each half.
  - The CLI has a new pane kind, `preview`.
  - Details are in the [two-pane finder plan](2026-11-05-finder-divider.md).

---

### Sources

Every measurement behind this plan is recorded in
[`docs/findings/2026-11-03-dependency-licenses.md`](../findings/2026-11-03-dependency-licenses.md):
- crate versions and licenses
- zellij's and tmux's dependencies
- the upstream repos' license and NOTICE files
- `alacritty_terminal`'s modules
- the size of the prototype
- which tools are installed locally

iTerm2 image support in `ratatui-image` is recorded in
[`docs/findings/2026-11-03-iterm2-image-support.md`](../findings/2026-11-03-iterm2-image-support.md).
