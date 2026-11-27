<!-- Generated from termos's command definitions: edit the help text in crates/tos/src, then run
     TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs -->

# `tos update`

Install the newest release over this one (only a tos installed by the one-line installer)

```
tos update [OPTIONS]
```

## Options

| Option | |
|---|---|
| `--force` | Install a --version from before `tos update` existed (0.2.x), after which only the one-line installer can bring you forward. |

## Examples

```sh
tos update                    # the newest release; nothing if this is it
tos update --version 0.3.0    # a particular one, for example to go back
The installer keeps the version before, so a session still running it is unaffected. Afterwards, tos update names the sessions still running and how to move them to the new version: detach (Ctrl+O d), `tos kill-all -y`, then attach again, which restores them.
```

See also: [all commands](README.md) · [the overview](../cli.md)
