<!-- Generated from termos's command definitions: edit the help text in crates/tos/src, then run
     TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs -->

# `tos subscribe`

Print events as JSON lines until the session ends: tabs and panes opening, closing, renaming and gaining focus, mode changes, clients. With -p, also those panes' text whenever it changes. The session comes from `tos -s`

```
tos subscribe [OPTIONS]
```

## Options

| Option | |
|---|---|
| `-p, --pane-id <PANE_ID>` | Panes whose text to print when it changes (e.g. terminal_1 or 1). |
| `-f, --format <FORMAT>` | raw: pane text as plain lines; json: every event as a JSON line. Values: `raw`, `json`. Default: `raw`. |
| `-s, --scrollback [<SCROLLBACK>]` | Include the scrollback in the first delivery: all of it, or the last N lines. |
| `--ansi` | Keep colours and styles in pane text. |

## Examples

```sh
tos subscribe -f json                 # every event as a JSON line
tos subscribe -p terminal_3           # pane 3's text whenever it changes
tos subscribe -p 3 -s 100 --ansi      # first with 100 lines of scrollback, in colour
```

See also: [all commands](README.md) · [the overview](../cli.md)
