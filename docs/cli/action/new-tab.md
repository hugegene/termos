<!-- Generated from termos's command definitions: edit the help text in crates/tos/src, then run
     TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs -->

# `tos action new-tab`

Open a new tab running a shell, or COMMAND; prints the tab's id

```
tos action new-tab [OPTIONS] [COMMAND]...
```

## Arguments

| Argument | |
|---|---|
| `[COMMAND]...` |  |

## Options

| Option | |
|---|---|
| `-n, --name <NAME>` |  |
| `-c, --cwd <CWD>` |  |
| `--start-suspended` | Type COMMAND at a shell's prompt without running it. |
| `--close-on-exit` | Accepted for zellij: tos closes a pane when its command exits. |

## Examples

```sh
tos action new-tab -n build -c ~/project -- make watch
tos action new-tab --start-suspended -- make deploy   # typed, not run
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
