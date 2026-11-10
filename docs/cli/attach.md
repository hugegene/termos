<!-- Generated from termos's command definitions: edit the help text in crates/tos/src, then run
     TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs -->

# `tos attach`

Attach to a session, starting it if needed (the default)

```
tos attach [OPTIONS] [NAME]
```

Also: `a`.

## Arguments

| Argument | |
|---|---|
| `[NAME]` | Session name (default: the session you're in, else "main"). |

## Options

| Option | |
|---|---|
| `-s, --session <SESSION>` | Session name (default: the session you're in, else "main"). |
| `-c, --create` | Create the session if it doesn't exist (always the case in tos; accepted for zellij's habit). |
| `-b, --create-background` | Start the session in the background without attaching. |
| `--forget` | Forget the saved layout first, so the session starts fresh. |
| `-f, --force-run-commands` | When restoring, run the commands that were running instead of waiting for Enter. |

## Examples

```sh
tos attach work            # attach to "work", starting it if needed
tos attach -b work         # start it in the background, without attaching
tos attach --forget work   # start fresh, without the saved layout
```

See also: [all commands](README.md) · [the overview](../cli.md)
