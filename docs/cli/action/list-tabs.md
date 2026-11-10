<!-- Generated from termos's command definitions: edit the help text in crates/tos/src, then run
     TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs -->

# `tos action list-tabs`

List tabs

```
tos action list-tabs [OPTIONS]
```

## Options

| Option | |
|---|---|
| `-j, --json` | Print JSON. |
| `-a, --all` | Every column. |
| `-d, --dimensions` | Size columns. |
| `-l, --layout` | The swap-layout column. |
| `-p, --panes` | The pane-count column. |
| `-s, --state` | Active, fullscreen and sync columns. |

## Examples

```sh
tos action list-tabs -s
tos action list-tabs -j | jq '.[] | select(.active) | .tab_id'
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
