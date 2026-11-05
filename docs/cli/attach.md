<!-- Generated from tb's command definitions: edit the help text in crates/tb/src, then run
     TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs -->

# `tb attach`

Attach to a session, starting it if needed (the default)

```
tb attach [OPTIONS] [NAME]
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
| `-c, --create` | Create the session if it doesn't exist (always the case in tb; accepted for zellij's habit). |
| `-b, --create-background` | Start the session in the background without attaching. |
| `--forget` | Forget the saved layout first, so the session starts fresh. |
| `-f, --force-run-commands` | When restoring, run the commands that were running instead of waiting for Enter. |

## Examples

```sh
tb attach work            # attach to "work", starting it if needed
tb attach -b work         # start it in the background, without attaching
tb attach --forget work   # start fresh, without the saved layout
```

See also: [all commands](README.md) · [the overview](../cli.md)
