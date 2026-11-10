<!-- Generated from termos's command definitions: edit the help text in crates/tos/src, then run
     TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs -->

# `tos action set-status`

Show TEXT in the status bar (tos). Each --id is its own segment; clicks reach `tos subscribe`

```
tos action set-status [OPTIONS] <TEXT>
```

## Arguments

| Argument | |
|---|---|
| `<TEXT>` |  |

## Options

| Option | |
|---|---|
| `--id <ID>` | The segment to set (one per extension). Default: `default`. |
| `--ttl <TTL>` | Remove the text after this many seconds. |

## Examples

```sh
tos action set-status --id ci "build ✓"
tos action set-status --id prbot --ttl 60 "PR #487 needs review"
tos action clear-status --id ci
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
