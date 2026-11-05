<!-- Generated from tb's command definitions: edit the help text in crates/tb/src, then run
     TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs -->

# `tb action send-keys`

Send keys, e.g. "Ctrl a" "F1" "Alt Shift b" Enter

```
tb action send-keys [OPTIONS] <KEYS>...
```

## Arguments

| Argument | |
|---|---|
| `<KEYS>...` |  |

## Options

| Option | |
|---|---|
| `-p, --pane-id <PANE_ID>` | The pane, e.g. terminal_3 or 3 (default: the focused pane). |

## Examples

```sh
tb action send-keys "Ctrl c"
tb action send-keys -p 3 Up Enter
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
