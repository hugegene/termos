# Finding: clicking a PNG in iTerm2 jumps the finder to its last file (2026-11-10)

## Question

On the user's Mac, in iTerm2, clicking a PNG in the finder's file list made the selection jump to the bottom
of the list. Nothing like it happens on Linux. What sends the finder to its last file, and is it ours to fix?
The fix depends on which program does what: the finder's click code, the image escape we send, or iTerm2.

## Method

- **iTerm2's source** at commit `8d54ab0b4141ab47a31ccb518691f705c4b4659d` (master, 2026-11-10), fetched with
  `gh api repos/gnachman/iTerm2/contents/<path> -H "Accept: application/vnd.github.raw"`:
  - `sources/InlineImages/KittyImageCommand.swift`: how a kitty command's keys are read
  - `sources/InlineImages/KittyImageController.swift`: transmit, display, and the answers it sends
  - `sources/InlineImages/VT100InlineImageHelper.m`: the iTerm2 protocol (`OSC 1337 File=`), for comparison
- **The kitty spec** at commit `0273bc12c3dcc835f4da4a0ea88233ace073706e`, `docs/graphics-protocol.rst`.
- **crossterm 0.29.0** (our client's input parser): `src/event/sys/unix/parse.rs` in `~/.cargo/registry`.
- **tos 0.3.1:** `crates/tos/src/server/images.rs`, `crates/tos/src/client.rs`, `crates/tos/src/server/finder/mod.rs`.
- **A throwaway e2e test** on Linux (Ubuntu, kernel 6.8): the harness in `crates/tos/tests/e2e.rs` opened a
  finder on `a.txt`, `b.txt`, `c.txt`, typed iTerm2's answer into `tos attach`, and waited for the preview of
  `c.txt`. The test was removed afterwards, since it shows the client's behaviour rather than checking the fix:

  ```rust
  s.wait_for("AAA-first");                // a.txt is selected
  s.send(b"\x1b_Gi=0;OK\x1b\\");          // what iTerm2 answers
  let text = s.wait_for("CCC-last");      // c.txt, the last file, is selected
  ```

  Run with `cargo test --offline -p tos --test e2e <name> -- --nocapture`.

## Results

**1. tos sends `q=2` only on the first chunk.** A picture's PNG is cut into chunks of 4096 base64 bytes
(`images.rs`, before the fix):

```
ESC _G a=T,f=100,t=d,i=7,c=40,r=20,C=1,q=2,m=1 ; <4096 bytes> ESC \
ESC _G m=1 ; <4096 bytes> ESC \
ESC _G m=0 ; <the rest> ESC \
```

**2. In iTerm2, a chunk without `q` means "answer me".** `KittyImageCommand.swift`: a chunk's action defaults
to `t` (transmit) and its verbosity to `normal`:

```swift
if let verbosity = extractEnum(from: dict, key: "q", defaultValue: Verbosity.normal) {
...
if let action = extractEnum(from: dict, key: "a", defaultValue: Action.transmit) {
```

**3. iTerm2 answers the last chunk by the last chunk's own `q`.** `KittyImageController.swift`,
`executeTransmit`. The last chunk is a plain transmit, so `display` is nil. The quiet first chunk is used
inside `reallyExecuteTransmit`, but this outer answer uses `modifiedCommand`, the last chunk:

```swift
let error = reallyExecuteTransmit(modifiedCommand, payload: payload, query: query)
if error != nil || display == nil {
    respondToTransmit(modifiedCommand, display: display, error: error)
}
```

`respondToTransmit` sends nothing while more chunks are expected. For the last chunk it sends:

```swift
let payload = args.joined(separator: ",") + semi + (error ?? "OK")   // "i=0;OK"
let message = "\u{1B}_G\(payload)\u{1B}\\"
switch imageTransmission.verbosity {
case .normal:
    delegate?.kittyImageControllerReport(message: message)
```

**4. crossterm reads that answer as keys.** `parse.rs`: ESC followed by anything other than `[`, `O` or ESC
is parsed as the next key with Alt added. So `ESC _ G i = 0 ; O K ESC \` becomes Alt+`_`, `G`, `i`, `=`,
`0`, `;`, `O`, `K`, Alt+`\`:

```rust
_ => parse_event(&buffer[1..], input_available).map(|event_option| {
    ...
        alt_key_event.modifiers |= KeyModifiers::ALT;
```

**5. `G` in the file list selects the last file** (`finder/mod.rs`):

```rust
KeyCode::Char('G') | KeyCode::End => self.select(self.visible.len().saturating_sub(1)),
```

None of the other keys does anything in the list, and tos binds neither Alt+`_` nor Alt+`\`.

**6. The throwaway e2e test passed:** after the answer was typed, the screen showed `c.txt`:

```
┌ /tmp/.tmp5OUa5B/r ──────────────┐┌ c.txt ─────────────────────── text · 9 B ┐
│ a.txt                           ││ 1 │ CCC-last                             │
│ b.txt                           ││                                          │
│ c.txt                           ││                                          │
```

**7. The kitty spec allows `q` on later chunks** (`graphics-protocol.rst`, line 488):

> Subsequent chunks **must** have only the ``m`` and optionally ``q`` keys.

**8. The iTerm2 protocol isn't involved.** `VT100InlineImageHelper.m` (`writeBaseCharacter`) writes the image
rows with a line feed between them, leaves the cursor on the last row, and sends no answer.

### The whole path, for one click

```
 user's Mac
┌──────────────────────┐ 1 click on cat.png ┌─────────────────────┐ 2 mouse event ┌──────────────────────┐
│ iTerm2               │───────────────────▶│ tos attach (client) │──────────────▶│ tos server           │
│                      │                    │ crossterm parser    │               │ finder selects       │
│                      │ 3 kitty chunks,    │                     │               │ cat.png, encodes it  │
│                      │◀───────────────────┼─────────────────────┼───────────────│ for kitty            │
│ answers last chunk:  │   q=2 on the first │                     │               │                      │
│ ESC _G i=0;OK ESC \  │ 4 the answer       │ parses it as keys:  │ 5 Alt+_, G, i │ G: select the last   │
│                      │───────────────────▶│ Alt+_ G i = 0 ; O K │──────────────▶│ file                 │
└──────────────────────┘   as typed input   └─────────────────────┘    ... keys   └──────────────────────┘
```

## Conclusions

- **The cause is the missing `q=2` on later chunks.** iTerm2 differs from the spec's intent here, but the spec
  allows `q` on every chunk. tos now sends `m=…,q=2` on each chunk (`images.rs`, test `kitty_sends_chunks`),
  so iTerm2 has nothing to answer.
- **The protocol choice is inferred, not seen:** iTerm2 has a kitty graphics implementation, so it presumably
  answered the kitty query at attach, and tos picked kitty over the iTerm2 protocol. The session log
  (`~/.local/state/termos/<session>.log`) records it as `client N shows images with Graphics { protocol: … }`.
- **Still unknown:**
  - Which iTerm2 version the user runs. The fix doesn't depend on it.
  - Whether kitty and Ghostty answer a last chunk without `q`. They weren't checked. The user hasn't seen this
    on Linux, so they likely keep the first chunk's `q`. Sending `q=2` on every chunk is safe for them either way.
- **A wider risk remains:** any answer a terminal sends after attach still reaches panes and the finder as
  keys. Swallowing `ESC _ … ESC \` in the client isn't simple, because Alt+`_` is also a real key (the
  shell's "insert last argument").
