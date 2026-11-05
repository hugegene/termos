# Finding: a KDL parser for `config.kdl` (2026-11-05)

## Question

Step 6 reads key bindings from `~/.config/tb/config.kdl` in zellij's keybind syntax
([CLI extension plan](../plans/2026-11-05-cli-extensions.md), item 7). Is a KDL parsing crate available to the
offline build, and which parts of KDL do real zellij configs use? The choice between a crate and our own
parser depends on it.

## Method

On the dev machine, on 2026-11-05:

```sh
ls ~/.cargo/registry/src/*/ | grep -i -E '^kdl|^knuffel|^miette'   # crates already downloaded
ls ~/.cargo/registry/cache/*/ | grep -i kdl
grep -c '' ~/.config/zellij/config.kdl
grep -n -E '^\s*(bind|keybinds|unbind|shared|shared_except|shared_among)' ~/.config/zellij/config.kdl | head
grep -n -E '/\*|r#"|#true|#false|\\$' ~/.config/zellij/config.kdl | head
```

## Results

- No `kdl`, `knuffel` or `miette` crate is in the local registry. Using one would need a download, which
  Claude's shell can't do; the user would have to run `cargo fetch`.
- The user's `config.kdl` is 585 lines. Its keybinds use:

```
keybinds clear-defaults=true {
        bind "Ctrl g" { SwitchToMode "normal"; }
        bind "left" { MoveFocus "left"; }
        bind "c" { SwitchToMode "renamepane"; PaneNameInput 0; }
        bind "d" { NewPane "down"; SwitchToMode "normal"; }
```

  plus mode blocks (`locked {`, `pane {`, …), `shared_except "locked" {`, binds with several keys
  (`bind "Alt left" "Alt h" { … }`), plugin blocks with child settings
  (`LaunchOrFocusPlugin "zellij:about" { floating true; … }`), and `//` comments.
- No `/* */` comments, raw strings (`r#"…"#`), KDL v2 keywords (`#true`) or line continuations (`\`).

## Conclusions

- **`tb` gets its own small KDL parser,** as it did its own image encoders. It covers what zellij configs use:
  - nodes with string, number and boolean arguments
  - `key=value` properties
  - child blocks, `;` and newlines as separators
  - `//` and `/* */` comments, `/-` to comment out a node
  - raw strings, and KDL v2's `#true`/`#false`/`#null`
- **Cost:** about 250 lines, an **estimate**, with no new dependency and no licence question.
- **Unknown:** whether other people's zellij configs use KDL features beyond this. The parser reports the
  line of anything it doesn't understand.
