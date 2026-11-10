<!-- Generated from termos's command definitions: edit the help text in crates/tos/src, then run
     TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs -->

# `tos delete`

Forget a saved session so it isn't restored (default: the current session name)

```
tos delete [OPTIONS] [NAME]
```

Also: `delete-session`, `d`.

## Arguments

| Argument | |
|---|---|
| `[NAME]` |  |

## Options

| Option | |
|---|---|
| `-s, --session <SESSION>` | Session name (default: the session you're in, else "main"). |
| `-f, --force` | Also end it if it's running. |

See also: [all commands](README.md) · [the overview](../cli.md)
