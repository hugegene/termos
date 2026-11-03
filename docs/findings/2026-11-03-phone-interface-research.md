# Finding: phone interface options (desk research)

**Date:** 2026-11-03 · **Used by:** [phone interface plan](../plans/2026-11-03-phone-interface.md) ·
**Companion measurement:** [prototype at phone sizes](2026-11-03-phone-terminal-measurements.md)

## Question

How should users reach a running session from a phone? The candidates:
- **(A)** an existing SSH terminal app
- **(B)** a PWA, either a web terminal or a custom phone UI fed by structured data
- **(C)** a native app

To choose, we needed the capabilities of each, the platform limits, the building blocks and their licences,
mobile network latency, and what comparable products chose.

## Method

- **Desk research:** web search and page fetches by a research agent, with GitHub repos and release notes
  read through the GitHub API. Every claim carries a source URL and is marked **V** (the source says so) or
  **I** (inferred).
- **Independent spot-check:** zellij v0.45.0's release notes, checked separately through the GitHub API.
  They were published 2026-08-20 and contain a section "Mobile Web UI + PWA". Confirmed.
- **Limits:** no phone was used. Device behaviour (screen size, how long a backgrounded connection
  survives) is **not measured** here.

## Results

### 1. Phone SSH apps

| | Termux | Blink Shell | Termius | JuiceSSH | ConnectBot |
|---|---|---|---|---|---|
| Platform | Android (local terminal; you run `ssh`/`mosh` inside) V | iOS/iPadOS V | iOS, Android, desktop V | Android V | Android V |
| Licence / price | Free, GPLv3 V | Free; Blink+ $19.99; GPL-3.0 V | Free tier; Pro $15/mo or $119/yr V | Last release Feb 2021; delisted from Play Dec 2025 V | Free, Apache-2.0 V |
| Mosh | Yes (1.4) V | Yes V | Yes V | Old only V | Yes (v1.11.0, Oct 2026) V |
| Tap → mouse click | Yes V | Yes V | Not documented | ? | ? |
| OSC 52 clipboard | Yes V | Probably I | Yes (iOS 7.4.0, Jun 2026) V | ? | ? |
| Inline images | Sixel/iTerm2 merged Sep 2026, **not yet released** V | iTerm2 only, plain SSH only (**not through tmux/mosh**) V | Not documented | ? | iTerm2 + kitty V |
| Extra key row | Yes (Esc, Ctrl, Alt, arrows…) V | "Smart Keys" V | Customisable bar V | ? | Yes V |
| Background | Foreground service; Android 12+ may kill it V | Only with a location workaround V | iOS stops it in **20–30 s** V | ? | Supported V |
| Push from our server | None I | Local alerts only I | Not documented | ? | Local only I |

**Other apps:**
- **Moshi** (iOS/Android) has a host hook and webhook API that send **push notifications** V.
- **Secure ShellFish** (iOS) has `notify`/`widget` commands V.
- **Mosh 1.4.0** (2022) supports OSC 52 and truecolor, but syncs only the visible screen, so there is no scrollback V.

**Phone terminal size:** no authoritative figures were found. One project says portrait is "~30–45 cols" V.
Arithmetic for a ~393 pt-wide phone (I):

| Orientation | Keyboard down | Keyboard up |
|---|---|---|
| Portrait | ~45–55 × 42–49 | ~45–55 × 19–23 |
| Landscape | ~90–105 cols | ~90–105 × 8–10 |

**Sources:**
- termux/termux-app (LICENSE.md, TerminalEmulator.java, TerminalView.java, PR #2973, releases); f-droid.org/en/packages/com.termux
- apps.apple.com Blink id1594898306; github.com/blinksh/blink; docs.blink.sh (tips-and-tricks, notifications)
- apps.apple.com Termius id549039908; docs.termius.com (changelog/ios, mobile-terminal, background FAQ)
- termai.sh/blog/juicessh-removed-play-store; mailman.mit.edu mosh-users 2024-02
- github.com/connectbot/connectbot releases v1.11.0
- github.com/mobile-shell/mosh release 1.4.0; mosh.org
- getmoshi.app/docs/notifications; apps.apple.com Secure ShellFish id1336634154
- cybersader.github.io/portagenty/getting-started/termux

### 2. Web terminals (xterm.js) on phones

- **Android soft-keyboard input:** garbled or duplicated input with Gboard, open since 2022 (#3600). Still
  reported in Apr 2026 V.
- **Touch, selection, scroll:** these fail or fall back to mouse emulation (#5377, open since Jul 2025) V.
  - The maintainers call the frontend "very desktop centered" V.
  - They call IME input "out of scope" and "not a priority" V.
  - Scrolling regressed in v6.0.0 V.
- **zellij's web client** uses xterm.js V, plus its own hidden-input keyboard layer, Gboard backspace
  workaround and touch handling.
  - **zellij 0.45.0 (2026-08-20)** added a **"Mobile Web UI + PWA"**: pane and tab pickers, an on-screen
    keyboard, and single-pane fullscreen. Verified independently.
- **xterm.js image addon:** sixel and iTerm2 are "beta"; kitty is partial, "alpha" V.
- **Other web terminals built on xterm.js:** VS Code, JupyterLab, ttyd, Wetty, Tailscale's web SSH client V.

**Sources:**
- github.com/xtermjs/xterm.js issues #3600 and #5377, and addons/addon-image
- github.com/zellij-org/zellij zellij-client/assets (soft-keyboard.js, touch.js) and release v0.45.0
- tsl0922/ttyd and butlerx/wetty `package.json`; tailscale/tailscale cmd/tsconnect

### 3. PWA limits (2025–2026)

**iOS:**
- **Web Push:**
  - only for **Home Screen** web apps (16.4+), with permission requested after a user gesture V
  - no developer account needed V
  - **every push must show a notification**; it is not for silent background work V
  - Declarative Web Push arrived in 18.4 V
- **Background:** apps are suspended shortly after they leave the foreground, and their connections stop
  V. A PWA's WebSocket will drop, so it must reconnect and resync I.
- **Install:** iOS 26 opens any Add-to-Home-Screen site as a web app V.
- **Keyboard:** `interactive-widget` is unsupported on iOS Safari through 27.2 V, so keyboard resizing must
  be handled by hand with `visualViewport` I.
- **Clipboard:** needs HTTPS and a user gesture; reading shows a "Paste" callout V.
- **Storage:** least-recently-used eviction, including after inactivity; Home Screen apps fare better V.
  Treat local data as a cache I.
- **EU:** Home Screen web apps remain available V.

**Android:**
- Chrome freezes background pages after **5 minutes** V.
- `interactive-widget` is supported V.

**No raw SSH or TCP from a phone browser:**
- Options are a WebSocket gateway (what ttyd, Wetty and zellij do) V, or WebTransport (Safari 26.4+) V. Both
  still need a server endpoint.
- Direct Sockets works only in desktop Chrome Isolated Web Apps; ChromeOS is enterprise-only and Android is excluded V.
- Tailscale's browser SSH console runs the whole client in WebAssembly (beta) V.

**Sources:**
- webkit.org/blog 13878, 12945, 16574, 10855, 14403, 17862
- developer.apple.com/forums/thread/716118
- heise.de/-10749652; caniuse `interactive-widget`; MDN VirtualKeyboard and BeforeInstallPromptEvent
- macrumors 2024-03-01
- chromium blink-dev threads (NOhYFW0rifw, 5R0P_aYBWQI, 5cSZjgnJelk)
- tailscale.com/kb/1216

### 4. Native app building blocks

| Component | Licence |
|---|---|
| SwiftNIO SSH | Apache-2.0 V |
| Citadel (Swift) | MIT V |
| libssh2 | BSD-3 V |
| sshj | Apache-2.0 V |
| mwiede/jsch | BSD-style V |
| dartssh2 (Flutter) | MIT V |
| **SwiftTerm** (iOS terminal view: sixel, kitty, mouse, OSC 8) | MIT V |
| **ConnectBot termlib** (Android Compose view on libvterm) | Apache-2.0 V |
| xterm.dart (Flutter; last push Jun 2025) | MIT V |
| **Termux terminal-emulator / terminal-view** | **Ambiguous:** repo GPLv3 with a partial Apache exception. Treat as GPL for a closed app I |

**Push:** FCM is free V; APNs is included with the Apple developer membership I.

**Stores:**
- **Costs:** Apple $99/yr V. Google $25 once, and new personal accounts need a 12-tester, 14-day closed test V.
- **Review risk:** App Store guideline **4.2.7** says remote clients that mirror specific software are
  limited to a user-owned host on the same LAN, and "thin clients for cloud-based apps are not appropriate" V.
  - Generic SSH apps are accepted V.
  - Whether 4.2.7 applies to a client for our own server is **unclear** I. It is lower risk if the app
    renders its own UI from an API (like any service client), and higher if it streams a remote screen I.

**Sources:** each repo's GitHub licence metadata; firebase.google.com/products/cloud-messaging;
developer.apple.com/programs/whats-included; Google Play help 6112435 and 14151465;
developer.apple.com/app-store/review/guidelines.

### 5. Mobile round-trip latency

- **UK** (Ofcom / Opensignal, Oct 2024–Mar 2025): median **5G 18.2 ms, 4G 21.1 ms** V.
- **US** (Ookla H2 2025): best carrier **46 ms** overall V.
- Both measure to nearby test servers. Phone → home machine adds the home uplink.
  - **Estimate (I):** SSH keystroke echo is ~25–60 ms typical, with worse spikes during handovers.
  - Mosh's predictive local echo hides this V.

**Sources:** 5g.co.uk/news 5570; advanced-television.com ?p=266943; rcrwireless.com 20260203; mosh.org.

### 6. What comparable products chose

| Product | Phone approach |
|---|---|
| Termius, Blink, Prompt | Native **terminal** apps with key bars V |
| Moshi, Secure ShellFish | Terminal **plus host-side push notifications** V |
| zellij 0.45 | **Web terminal + dedicated mobile UI + PWA** V |
| Tailscale SSH console | Browser terminal (WebAssembly + xterm.js) V |
| code-server / Coder | PWA on iPad; known issues: keyboard disappears, poor terminal copy/paste V |
| GitHub Mobile | **Native UI, no terminal**; assign the coding agent and review PRs V |
| Claude Code mobile | **Native client UI** with remote control and push, not a terminal V |
| Codespaces | No mobile app V |

**Sources:** the store and docs pages above; github.blog changelog 2025-06-03; code.claude.com/docs/en/mobile;
coder.com/docs/code-server/ipad; github.com/orgs/community/discussions/135358.

## Could not verify

- Truecolor, OSC 8 and tap-to-mouse support in Termius, JuiceSSH and ConnectBot.
- How long a WebSocket survives in a backgrounded iOS Home Screen app.
- The reported "3 silent pushes → revoked" rule for iOS Web Push.
- Any documented phone column/row sizes.
- Whether Warp has a mobile app.
- Android sideloading developer-verification rules (sources unavailable).

## Conclusions

1. **The live session must stay on the server, and alerts need their own path.** On iOS, no app or PWA keeps
   a connection in the background without workarounds (Termius 20–30 s; PWAs suspended). Every product that
   solves this holds the session server-side and sends alerts by push (Moshi, ShellFish, Claude Code).
2. **Terminal apps are a dependable baseline, but not a rich viewer.** Mosh, extra keys and tap-to-mouse are
   common. Images and clipboard support are uneven, and images don't get through Mosh.
3. **A web terminal on a phone is hard.** xterm.js mobile input isn't a maintainer priority. zellij needed
   its own keyboard and touch layer plus a separate mobile UI to make it work. We shouldn't make a web
   terminal our main phone experience.
4. **A PWA with its own UI is viable within known limits:**
   - push only once it's on the Home Screen, with a visible notification for every push
   - reconnect after being backgrounded
   - handle the keyboard manually on iOS
   - local data is a cache
   - it needs an HTTPS/WebSocket endpoint on our side
5. **Native is affordable but brings review risk and a second codebase.** It costs $99/yr + $25. Permissive
   building blocks exist (SwiftTerm, ConnectBot termlib, SSH libraries); avoid the Termux libraries.
   Guideline 4.2.7 is a risk mainly for screen-streaming designs.
6. **Mobile latency** (18–46 ms medians) makes a terminal usable but every keystroke waits on the network.
   A local UI removes the wait. This agrees with the companion measurement (one network round trip per action).
