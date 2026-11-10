<!-- Generated from termos's command definitions: edit the help text in crates/tos/src, then run
     TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs -->

# `tos action go-to-tab-name`

Go to the tab with this name

```
tos action go-to-tab-name [OPTIONS] <NAME>
```

## Arguments

| Argument | |
|---|---|
| `<NAME>` |  |

## Options

| Option | |
|---|---|
| `-c, --create` | Create it if there's none. |

## Examples

```sh
tos action go-to-tab-name logs
tos action go-to-tab-name -c scratch   # create it if it's missing
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
