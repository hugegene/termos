<!-- Generated from tb's command definitions: edit the help text in crates/tb/src, then run
     TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs -->

# `tb action list-panes`

List panes

```
tb action list-panes [OPTIONS]
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
tb action list-panes -t -c
tb action list-panes -j | jq '.[] | select(.is_focused) | .id'
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
