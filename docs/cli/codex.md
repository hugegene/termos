<!-- Generated from termos's command definitions: edit the help text in crates/tos/src, then run
     TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs -->

# `tos codex`

Start OpenAI's Codex CLI with the docs pane's hook (Codex asks once to trust it)

```
tos codex [ARGS]...
```

## Arguments

| Argument | |
|---|---|
| `[ARGS]...` | Arguments for the agent, passed on as they are. |

## Examples

```sh
tos claude                    # Claude Code, with the docs pane's hook
tos claude --resume           # every argument goes to the agent
tos codex "plan the export"   # Codex, starting with that prompt
tos claude --help             # Claude's own help
```

See also: [all commands](README.md) · [the overview](../cli.md)
