<!-- Generated from tb's command definitions: edit the help text in crates/tb/src, then run
     TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs -->

# `tb action move-pane`

Swap a pane with its neighbour in a direction, or with the next pane

```
tb action move-pane [OPTIONS] [DIRECTION]
```

## Arguments

| Argument | |
|---|---|
| `[DIRECTION]` | Values: `left`, `right`, `up`, `down`. |

## Options

| Option | |
|---|---|
| `-p, --pane-id <PANE_ID>` | The pane, e.g. terminal_3 or 3 (default: the focused pane). |

See also: [all commands](../README.md) · [the overview](../../cli.md)
