<!-- Generated from tb's command definitions: edit the help text in crates/tb/src, then run
     TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs -->

# `tb action resize`

Resize a pane: `increase|decrease [left|right|up|down]` (zellij), or `DIRECTION [CELLS]` (tb)

```
tb action resize [OPTIONS] <HOW> [SIDE]
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
tb action resize increase left
tb action resize decrease          # every side
tb action resize down 5            # tb's form: move the border 5 cells
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
