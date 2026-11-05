<!-- Generated from tb's command definitions: edit the help text in crates/tb/src, then run
     TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs -->

# `tb dump-screen`

Print a pane's text (default: the focused pane)

```
tb dump-screen [OPTIONS]
```

## Options

| Option | |
|---|---|
| `-s, --session <SESSION>` | Session name (default: the session you're in, else "main"). |
| `-p, --pane-id <PANE_ID>` | The pane, e.g. terminal_3 or 3 (default: the focused pane). |
| `-f, --full` | Include the scrollback. |
| `-a, --ansi` | Keep colours and styles (ANSI escape sequences). |
| `--path <PATH>` | Write to this file instead of printing. |

See also: [all commands](README.md) · [the overview](../cli.md)
