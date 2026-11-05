<!-- Generated from tb's command definitions: edit the help text in crates/tb/src, then run
     TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs -->

# `tb delete`

Forget a saved session so it isn't restored (default: the current session name)

```
tb delete [OPTIONS] [NAME]
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
