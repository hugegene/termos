<!-- Generated from tb's command definitions: edit the help text in crates/tb/src, then run
     TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs -->

# `tb action new-tab`

Open a new tab running a shell, or COMMAND; prints the tab's id

```
tb action new-tab [OPTIONS] [COMMAND]...
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
| `--close-on-exit` | Accepted for zellij: tb closes a pane when its command exits. |

## Examples

```sh
tb action new-tab -n build -c ~/project -- make watch
tb action new-tab --start-suspended -- make deploy   # typed, not run
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
