<!-- Generated from tb's command definitions: edit the help text in crates/tb/src, then run
     TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs -->

# `tb action go-to-tab-name`

Go to the tab with this name

```
tb action go-to-tab-name [OPTIONS] <NAME>
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
tb action go-to-tab-name logs
tb action go-to-tab-name -c scratch   # create it if it's missing
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
