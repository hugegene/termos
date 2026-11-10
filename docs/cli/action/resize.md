<!-- Generated from termos's command definitions: edit the help text in crates/tos/src, then run
     TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs -->

# `tos action resize`

Resize a pane: `increase|decrease [left|right|up|down]` (zellij), or `DIRECTION [CELLS]` (tos)

```
tos action resize [OPTIONS] <HOW> [SIDE]
```

## Arguments

| Argument | |
|---|---|
| `<HOW>` |  |
| `[SIDE]` |  |

## Options

| Option | |
|---|---|
| `-p, --pane-id <PANE_ID>` | The pane, e.g. terminal_3 or 3 (default: the focused pane). |

## Examples

```sh
tos action resize increase left
tos action resize decrease          # every side
tos action resize down 5            # termos's form: move the border 5 cells
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
