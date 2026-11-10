<!-- Generated from termos's command definitions: edit the help text in crates/tos/src, then run
     TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs -->

# `tos action list-panes`

List panes

```
tos action list-panes [OPTIONS]
```

## Options

| Option | |
|---|---|
| `-j, --json` | Print JSON. |
| `-a, --all` | Every column. |
| `-c, --command` | Command and folder columns. |
| `-g, --geometry` | Position and size columns. |
| `-s, --state` | Focused, floating and exited columns. |
| `-t, --tab` | Tab columns. |

## Examples

```sh
tos action list-panes -t -c
tos action list-panes -j | jq '.[] | select(.is_focused) | .id'
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
