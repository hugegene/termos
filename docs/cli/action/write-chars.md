<!-- Generated from tb's command definitions: edit the help text in crates/tb/src, then run
     TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs -->

# `tb action write-chars`

Type text into a pane

```
tb action write-chars [OPTIONS] <CHARS>
```

## Arguments

| Argument | |
|---|---|
| `<CHARS>` |  |

## Options

| Option | |
|---|---|
| `-p, --pane-id <PANE_ID>` | The pane, e.g. terminal_3 or 3 (default: the focused pane). |
| `--enter` | Press Enter after the text (tb). |

## Examples

```sh
tb action write-chars --enter "make test"
tb action write-chars -p terminal_3 q
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
