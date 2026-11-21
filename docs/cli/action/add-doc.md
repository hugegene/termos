<!-- Generated from termos's command definitions: edit the help text in crates/tos/src, then run
     TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs -->

# `tos action add-doc`

Show a Markdown file in the docs pane of a pane's tab, opening the docs pane if needed (tos). `tos claude` and the other agent launchers do this for each .md file the agent writes

```
tos action add-doc [OPTIONS] <FILE>
```

## Arguments

| Argument | |
|---|---|
| `<FILE>` |  |

## Options

| Option | |
|---|---|
| `-p, --pane-id <PANE_ID>` | The pane whose tab gets the file, e.g. terminal_3 or 3 (default: the pane this runs in). |
| `--cwd <CWD>` | The folder the docs pane shows the file's path relative to (default: the current folder). |

## Examples

```sh
tos action add-doc docs/plans/2026-11-21-x.md          # beside the pane this runs in
tos action add-doc -p terminal_7 --cwd ~/termos notes.md
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
