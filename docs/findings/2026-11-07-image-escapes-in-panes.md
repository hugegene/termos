# Finding: what a terminal pane does with image escapes today

**Date:** 2026-11-07

## Question

Could programs in ordinary `tb` terminal panes show pictures, if the pane learned the image protocols? What
does the parser under our `alacritty_terminal` fork do today with sixel, kitty and iTerm2 escapes?

The decision that depends on it: how extensions get pictures. The options are pictures inside terminal panes, a
CLI that lays pictures over a pane, or a new kind of pane that a program draws through an API instead of escape
codes.

## Method

Read the source on this machine (Linux 6.8.0-147). The fork is in `crates/alacritty_terminal`, and `vte` 0.15.0
is a crates.io dependency (from `Cargo.lock`), not part of the fork.

```sh
grep -rn -i "sixel\|fn hook\|apc\|graphics" crates/alacritty_terminal/src
grep -n '^name = "vte"' -A1 Cargo.lock
V=~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vte-0.15.0
grep -rn -i "apc\|fn hook\|sos_pm_apc" $V/src
sed -n 1305,1340p $V/src/ansi.rs
sed -n 176,186p $V/src/lib.rs
grep -rn "catch_unwind\|panic::set_hook\|panic = " crates/tb/src Cargo.toml
ls ~/.cargo/registry/src/*/ | grep -i -E "sixel|icy"
```

## Results

- **The fork:** no matches for `sixel`, `fn hook`, `apc` or `graphics` in `crates/alacritty_terminal/src`.
- **vte 0.15.0, sixel (a DCS string, `ESC P … q … ESC \`):** the parser passes it to `hook`/`put`/`unhook`.
  The `ansi` layer that the fork uses only logs and drops it:
  ```
  vte-0.15.0/src/ansi.rs:1311:    fn hook(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
          debug!("[unhandled hook] params={:?}, ints: {:?}, ignore: {:?}, action: {:?}", ...);
      fn put(&mut self, byte: u8) {
          debug!("[unhandled put] byte={:?}", byte);
      fn unhook(&mut self) {
          debug!("[unhandled unhook]");
  ```
- **vte 0.15.0, kitty graphics (an APC string, `ESC _ G … ESC \`):** the parser enters `SosPmApcString` and
  skips bytes with no callback at all:
  ```
  vte-0.15.0/src/lib.rs:182:            State::SosPmApcString => self.anywhere(performer, byte),
  vte-0.15.0/src/lib.rs:377:            0x5E..=0x5F => self.state = State::SosPmApcString,
  ```
- **vte 0.15.0, iTerm2 (OSC 1337):** reaches `osc_dispatch` in `ansi.rs`, which handles known OSC numbers and
  logs the rest as unhandled.
- **What a pane tells programs about itself:** the fork answers device attributes (`ESC[c`) with `ESC[?6c`,
  which has no `4`, so programs that look for sixel support won't find it:
  ```
  crates/alacritty_terminal/src/term/mod.rs:1257:    fn identify_terminal(&mut self, intermediate: Option<char>) {
  crates/alacritty_terminal/src/term/mod.rs:1261:                let text = String::from("\x1b[?6c");
  ```
  (`grep -rn '?6c\|identify_terminal' crates/alacritty_terminal/src`)
- **Panics in the server:** no `catch_unwind`, panic hook or `panic =` profile setting in `crates/tb/src` or
  `Cargo.toml`.
- **Sixel crates on this machine:** none in the local cargo registry. The [licence
  finding](2026-11-03-dependency-licenses.md) lists `icy_sixel` 0.7.0 (MIT OR Apache-2.0), and zellij's
  `sixel-image` and `sixel-tokenizer` among zellij's dependencies.

## Conclusions

- **Pictures inside terminal panes need changes to `vte`, not only the fork.** Our own `Handler` never sees an
  image escape. We would have to vendor `vte` as we did `alacritty_terminal`, or catch the escapes before `vte`.
- **Sixel is the easiest of the three:** `vte` already splits it out through `hook`/`put`/`unhook`. iTerm2's OSC
  1337 is next. kitty would need `vte` to report APC strings at all.
- **Panes would also have to say they support sixel:** a `4` in the device-attributes reply. The [pane pixel
  sizes finding](2026-11-04-pane-pixel-sizes.md) adds that `vte` 0.15 drops the cell-size query `CSI 16 t`.
- **A panic in finder code ends the session server:** the finder runs on the server's main thread, and nothing
  catches panics.
- **Still unknown:**
  - which sixel decoder to use, and its licence and dependencies. None is fetched yet, so a new one needs
    `cargo fetch` from the user.
  - how much code it takes to keep a picture anchored to grid cells while the text scrolls.
