# Plan: phone interface

**Status:** proposal · **Date:** 2026-11-03 · **Part of:** [rebuild plan](2026-11-03-rebuild-plan.md) ·
**Findings:** [prototype at phone sizes](../findings/2026-11-03-phone-terminal-measurements.md) ·
[phone options research](../findings/2026-11-03-phone-interface-research.md)

## Decision

Use **two paths**. Neither of them re-styles SSH output.

1. **Baseline: any SSH terminal app**, supported from day one. Termius or Blink on iOS, Termux or ConnectBot
   on Android, with **Mosh**. The user runs `<product> attach`. All it needs from us is a **single-pane
   layout** in the TUI for screens below ~80 columns. We measured 40–50 columns as unusable two-pane and 100
   as fine; the exact cut-off is still to confirm. This is the path for shells, Neovim and anything interactive.
2. **Main phone product: a PWA with its own phone UI**, fed with **structured data** from our server. It covers:
   - browsing files and previewing them (real images, rendered Markdown, CSV tables)
   - editing text with the phone's own keyboard
   - the inbox of daemon events, plus **push notifications**
   - an overview of running sessions

   It is installable from the browser on iOS and Android, with no app store.

**Not now:**
- **A native app.** Build one only if one of the triggers in §6 is measured.
- **A web terminal (xterm.js) as the main phone experience.**

## Why

| Evidence | Points to |
|---|---|
| At 40–50 columns the current two-pane layout leaves 12–16 columns for file names (`….md`) [M§1] | The TUI needs a single-pane phone layout; a terminal is cramped for browsing |
| Each terminal interaction is a network round trip. Typing one character in Neovim sends 0.75–1.2 KB back; scrolling one line sends 1–6 KB [M§2] | Local UI for reading and editing; the terminal for commands |
| A real 400 px image is 8 KB, while the block-character preview is 12–44 KB raw and looks worse [M§3] | Previews belong in a phone UI |
| A terminal stream is a grid of characters with no meaning attached [M, conclusion 6] | "Display nicely" needs **structured data**, not re-styled SSH output |
| Mobile median RTT is 18–46 ms; Mosh hides it for typing [R§5] | The SSH baseline is usable with Mosh |
| xterm.js mobile input is "not a priority" upstream; zellij needed its own keyboard and touch layer plus a separate mobile UI [R§2] | Don't make a web terminal the main experience |
| iOS suspends apps and PWAs in the background; push needs a Home Screen install and a visible notification for every push [R§3] | Keep sessions on the server; send alerts by push; reconnect on resume |
| Browsers can't open SSH or TCP [R§3] | A PWA needs our own HTTPS/WebSocket endpoint |
| Native costs $99/yr + $25; App Store guideline 4.2.7 is a risk for screen-streaming apps; there are two codebases [R§4] | PWA first, native only when triggered |
| GitHub Mobile and Claude Code mobile are **native-UI clients, not terminals**; Moshi and ShellFish are **terminal + push** [R§6] | Same split: phone UI for content and alerts, terminal for commands |

M = [measurements](../findings/2026-11-03-phone-terminal-measurements.md),
R = [research](../findings/2026-11-03-phone-interface-research.md).

## 1. Options considered

| Option | Verdict |
|---|---|
| **A. SSH terminal app + Mosh** | ✅ **Baseline.** Free and works with any phone. Weak at images and browsing, and gets no push from us |
| **B1. PWA that shows the terminal** (xterm.js, or re-styling the SSH output) | ❌ As the main experience: phone input problems, no meaning to restyle, and still one round trip per key. zellij 0.45 already ships this if anyone wants it |
| **B2. PWA with its own UI over our structured API** | ✅ **Main phone product** |
| **C. Native app** (SwiftTerm on iOS, ConnectBot termlib on Android) | ⏸ **Later, if triggered** (§6). Same API, so nothing is wasted |
| B2 + a **read-only pane viewer and a "compose and send" box** | ✅ Phase P4. This is the workable version of "show the terminal nicely". The server sends a pane's text from its emulator; the phone shows it with wrapping and sends whole lines typed in a normal text box (autocorrect is fine) |

## 2. Architecture changes

```
phone PWA ──HTTPS + WebSocket──► phone API (private: tailnet-only + login)
                                   │ uses finder-core (list, previews, save)
                                   │ local socket ──► session server (sessions, pane text, pane input)
phone SSH app ──ssh / mosh──► `<product> attach` ──► session server
desktop terminal ─────────────────────────────────► session server
webhooks ──► webhook listener (public, signature-checked) ─┐
poller ─────────────────────────────────────────────────────┴─► events ─► session server (finder UI)
                                                                       └► Web Push ─► phone
```

1. **`finder-core` library (from rebuild step 1).** It holds file listing, type detection, previews as data
   and saving with conflict checks. Both the TUI and the phone API use it, so nothing is written twice.
   - **Previews for the phone:**
     - Markdown and HTML become sanitised HTML (`pulldown-cmark`, `htmd`).
     - Code becomes highlighted HTML (`syntect` has HTML output).
     - CSV is sent as paged rows.
     - Images are resized to JPEG or WebP (`image`).
2. **Phone API.** This is a second listener in the daemon (`axum`), **separate from the public webhook
   listener**:
   - It binds only to the Tailscale interface and needs a per-device login.
   - It is HTTPS, which Clipboard and Web Push require anyway.
   - It uses WebSockets for live updates; the client reconnects and catches up when the app comes back
     to the foreground.
3. **Saving edits.** Every save carries the file's version (mtime/etag). If the file changed elsewhere, for
   example in Neovim on the desktop, the phone shows "changed on another device" with a choice: keep mine,
   keep theirs, or compare. Drafts are saved to the server often, because iOS may evict local storage.
4. **Push.** Daemon events produce a Web Push notification. Every push is visible, as iOS requires, and
   tapping it opens the matching file or event. **There is always an in-app inbox too; push is never the only
   copy.**
5. **Security.** This changes the rebuild plan's "SSH is the only way in" (see its change log):
   - **Paths:** the phone API exposes only configured root folders.
   - **Devices:** each device is approved by `<product> pair` (a QR code shown on the desktop) and can be
     revoked.
   - **Audit:** every write and every pane input is logged.
   - **Pane input** (P4) is off by default and enabled per device.

## 3. PWA screens

1. **Home:** sessions and tabs (read from the session server), the event inbox, recent files.
2. **Files:** large tap targets, breadcrumbs, fuzzy search, sorting.
3. **Preview:** rendered Markdown, highlighted code, CSV table (sticky header, horizontal scroll), images
   with pinch-zoom, sanitised HTML, and a "source" toggle.
4. **Edit:** a plain text editor with a Markdown toolbar and a preview toggle; save with conflict check.
5. **Pane viewer (P4):** read-only text of a pane (follow mode for logs and long jobs), plus a compose box
   that sends a line to that pane.
6. **Settings:** devices, notification choices, root folders.

## 4. Roadmap

This slots into the rebuild roadmap. Sizes are **estimates**.

| Step | Scope | Depends on |
|---|---|---|
| **Rebuild step 1** | Build the finder logic as the `finder-core` library from the start (cheap now, costly to split later) | n/a |
| **Rebuild step 7** | Single-pane TUI layout below ~80 columns: the **SSH baseline** (was step 4; moved 2026-11-04; step 7 since 2026-11-05) | steps 1–3 |
| **P0. Device measurements** | The checks in §5, recorded in `docs/findings/` | can start now |
| **P1. Read-only PWA** | Phone API (listing, previews, sessions list) and PWA screens 1–3; tailnet + pairing | step 1 |
| **P2. Editing** | Screen 4, versioned saves, conflict view, server-side drafts | P1 |
| **P3. Events and push** | Inbox and Web Push from daemon events | P1 + daemon |
| **P4. Pane viewer + compose box** | Screen 5; pane text from the session server; per-device switch for input | P1 + step 2 |
| **P5. Native (only if triggered)** | Native shell over the same API; SwiftTerm / ConnectBot termlib if an in-app terminal is required | §6 trigger |

**Rough size:** P1–P3 together are about 4–6k lines (Rust API plus TypeScript PWA). This is an **estimate**.

## 5. Measurements still needed (P0)

Each of these gets recorded in `docs/findings/` when done.

1. **Real terminal sizes.** Run `stty size` in your SSH app at its default font: portrait and landscape,
   with the keyboard up and down. This replaces the assumed 40×18 and 50×36.
2. **Network round trip from phone to dev box.** Use `tailscale ping` (or `ping`), 20 samples each on
   Wi-Fi, 4G and 5G, and record the median and p95.
3. **Typing feel.** Compare `ssh` and `mosh` into Neovim over mobile data.
4. **zellij 0.45 mobile web UI as a benchmark.** Install it alongside the current zellij in
   `~/.local/opt`, without replacing 0.44.1, which runs your `prbot` session. Note what works and what's
   annoying.
5. **PWA spike on your phone:**
   - how long a WebSocket survives after switching apps
   - Web Push delivery time
   - keyboard and viewport behaviour in the editor screen
   - local storage after a few days unused

## 6. When a native app becomes worth it (triggers)

Build P5 only if at least one of these is **measured or required**:
- Web Push proves unreliable on iOS in practice: missed or late alerts in the P0/P3 measurements.
- Users need things a PWA can't do: Live Activities or widgets, share-sheet "open in", or biometric-locked
  keys.
- Interactive terminal use on the phone is common enough that sending people to an SSH app hurts.
- An app-store listing is a business requirement.

## 7. Risks

| Risk | Mitigation |
|---|---|
| Maintaining two UIs (TUI + PWA) | Shared `finder-core`; API-first; previews produced once on the server |
| A file read/write API reachable over the network | Tailnet-only binding, per-device pairing, root-folder allow-list, audit log, pane input off by default |
| iOS PWA limits (background, keyboard, eviction) | State lives on the server; resync on resume; manual `visualViewport` handling; server-side drafts |
| Edit conflicts between phone and desktop | Versioned saves and a conflict view; Neovim warns when a file changed on disk |
| Push unreliable or rate-limited | In-app inbox is the source of truth; push is a hint |
| Requiring Tailscale may put other users off | Open question below; possible alternatives are a tunnel with login, or our own relay later |

## 8. Open questions

1. **Which phones:** iOS, Android or both? That sets which SSH apps to document and test first.
2. **Remote access for other users:** is Tailscale acceptable as a requirement? The alternatives are a tunnel
   with login, or our own relay service, which is a business decision.
3. **Pane input from the phone:** should it run commands at all (P4), or stay view-only?
4. **Offline reading:** should cached files be readable offline?
5. **To verify before use:** licences for the PWA editor component, the Web Push and pairing/auth libraries,
   and an HTML sanitiser crate. Record these as a finding, per CLAUDE.md.

## Change log

- **2026-11-03:** first version.
- **2026-11-05:** the narrow layout is now rebuild **step 7**, because a new step 6 (CLI and extensions) comes
  first ([rebuild plan](2026-11-03-rebuild-plan.md) change log).
