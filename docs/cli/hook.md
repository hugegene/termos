<!-- Generated from termos's command definitions: edit the help text in crates/tos/src, then run
     TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs -->

# `tos hook`

What an agent's hook runs after a tool call, with the call as JSON on stdin: shows the .md files it wrote in the docs pane. Prints nothing and never fails

```
tos hook --agent <AGENT>
```

## Options

| Option | |
|---|---|
| `--agent <AGENT>` | The agent whose JSON this is. Values: `claude`, `codex`, `agy`, `agent`, `grok`, `gemini`. |

See also: [all commands](README.md) · [the overview](../cli.md)
