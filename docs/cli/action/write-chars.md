<!-- Generated from termos's command definitions: edit the help text in crates/tos/src, then run
     TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs -->

# `tos action write-chars`

Type text into a pane

```
tos action write-chars [OPTIONS] <CHARS>
```

## Arguments

| Argument | |
|---|---|
| `<CHARS>` |  |

## Options

| Option | |
|---|---|
| `-p, --pane-id <PANE_ID>` | The pane, e.g. terminal_3 or 3 (default: the focused pane). |
| `--enter` | Press Enter after the text (tos). |

## Examples

```sh
tos action write-chars --enter "make test"
tos action write-chars -p terminal_3 q
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
