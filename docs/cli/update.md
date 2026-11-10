<!-- Generated from termos's command definitions: edit the help text in crates/tos/src, then run
     TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs -->

# `tos update`

Install the newest release over this one (only a tos installed by the one-line installer)

```
tos update
```

## Examples

```sh
tos update                    # the newest release
tos update --version 0.2.1    # a particular one, for example to go back
The installer keeps the version before, so a session still running it is unaffected; `tos kill` and attaching again move it to the new one.
```

See also: [all commands](README.md) · [the overview](../cli.md)
