<!-- Generated from termos's command definitions: edit the help text in crates/tos/src, then run
     TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs -->

# `tos action new-pane`

Open a pane running a shell, or COMMAND; prints the pane's id

```
tos action new-pane [OPTIONS] [COMMAND]...
```

## Arguments

| Argument | |
|---|---|
| `[COMMAND]...` | The command to run (default: a shell). |

## Options

| Option | |
|---|---|
| `-d, --direction <DIRECTION>` | Open beside the focused pane (default: where there's most room). Values: `right`, `down`. |
| `-n, --name <NAME>` | Name the pane (shown in its frame). |
| `--cwd <CWD>` | The folder to start in (default: the focused pane's). |
| `-s, --start-suspended` | Type COMMAND at a shell's prompt without running it. |
| `-c, --close-on-exit` | Accepted for zellij: tos closes a pane when its command exits. |

## Examples

```sh
tos action new-pane -d right -- htop
tos action new-pane -n logs -- tail -f /var/log/syslog
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
