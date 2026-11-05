<!-- Generated from tb's command definitions: edit the help text in crates/tb/src, then run
     TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs -->

# `tb action set-status`

Show TEXT in the status bar (tb). Each --id is its own segment; clicks reach `tb subscribe`

```
tb action set-status [OPTIONS] <TEXT>
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
tb action set-status --id ci "build ✓"
tb action set-status --id prbot --ttl 60 "PR #487 needs review"
tb action clear-status --id ci
```

See also: [all commands](../README.md) · [the overview](../../cli.md)
