<!-- Generated from tb's command definitions: edit the help text in crates/tb/src, then run
     TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs -->

# `tb action set-pane-color`

Set a pane's default colours ("#00e000" or "rgb:00/e0/00"); default: the pane this runs in

```
tb action set-pane-color [OPTIONS]
```

## Options

| Option | |
|---|---|
| `-p, --pane-id <PANE_ID>` | The pane, e.g. terminal_3 or 3 (default: the focused pane). |
| `--fg <FG>` |  |
| `--bg <BG>` |  |
| `--reset` | Back to the terminal's colours. |

## Examples

```sh
tb action set-pane-color --bg "#3a0000"   # tint the pane this runs in
tb action set-pane-color -p 3 --reset
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
