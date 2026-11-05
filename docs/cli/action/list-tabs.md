<!-- Generated from tb's command definitions: edit the help text in crates/tb/src, then run
     TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs -->

# `tb action list-tabs`

List tabs

```
tb action list-tabs [OPTIONS]
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
tb action list-tabs -s
tb action list-tabs -j | jq '.[] | select(.active) | .tab_id'
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
