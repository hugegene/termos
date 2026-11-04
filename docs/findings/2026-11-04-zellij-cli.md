# Finding: zellij's session commands (2026-11-04)

## Question

The user asked whether `tb` can use the same commands as zellij for creating, attaching to and managing
sessions. Which commands, arguments and flags does the user's zellij have? The plan
[../plans/2026-11-04-zellij-cli.md](../plans/2026-11-04-zellij-cli.md) copies them.

## Method

On the dev machine (Ubuntu, Linux 6.8), on 2026-11-04, with the user's zellij:

```sh
/snap/bin/zellij --help
/snap/bin/zellij attach --help
for c in list-sessions kill-session delete-session kill-all-sessions delete-all-sessions; do
  /snap/bin/zellij help $c
done
```

`zellij <subcommand> --help` printed nothing for the last five (likely the snap wrapper), so they were read
with `zellij help <subcommand>`. Only the help text was read. Behaviour (for example, what `zellij -s NAME`
does when NAME exists) wasn't run.

## Results

`zellij 0.44.1`. Trimmed to the session commands:

```
zellij [OPTIONS] [SUBCOMMAND]
    -s, --session <SESSION>    Specify name of a new session
    -l, --layout <LAYOUT>      ... otherwise will start a new session
    -n, --new-session-with-layout <...>   Will always start a new session, even if inside an existing session

SUBCOMMANDS:
    action                 Send actions to a specific session [aliases: ac]
    attach                 Attach to a session [aliases: a]
    delete-all-sessions    Delete all sessions [aliases: da]
    delete-session         Delete a specific session [aliases: d]
    kill-all-sessions      Kill all sessions [aliases: ka]
    kill-session           Kill a specific session [aliases: k]
    list-sessions          List active sessions [aliases: ls]
    run                    Run a command in a new pane [aliases: r]
    edit                   Edit file with default $EDITOR / $VISUAL [aliases: e]
    watch                  Watch a session (read-only) [aliases: w]
    (also convert-config, convert-layout, convert-theme, list-aliases, options, pipe, plugin, setup,
     subscribe, web)

zellij attach [OPTIONS] [SESSION_NAME] [SUBCOMMAND]
    -b, --create-background     Create a detached session in the background if one does not exist
    -c, --create                Create a session if one does not exist
    -f, --force-run-commands    If resurrecting a dead session, immediately run all its commands on startup
        --forget                Delete saved session before connecting
        --index <INDEX>         Number of the session index in the active sessions ordered creation date
        --ca-cert, --insecure, -r/--remember, -t/--token   (remote sessions)
    SUBCOMMANDS: options

zellij list-sessions [OPTIONS]
    -n, --no-formatting    Do not add colors and formatting to the list (useful for parsing)
    -r, --reverse          List the sessions in reverse order (default is ascending order)
    -s, --short            Print just the session name

zellij kill-session [TARGET_SESSION]

zellij delete-session [OPTIONS] [TARGET_SESSION]
    -f, --force    Kill the session if it's running before deleting it

zellij kill-all-sessions [OPTIONS]
    -y, --yes     Automatic yes to prompts

zellij delete-all-sessions [OPTIONS]
    -f, --force    Kill the sessions if they're running before deleting them
    -y, --yes      Automatic yes to prompts
```

## Conclusions

- zellij names the session as a **positional argument** on `attach`, `kill-session` and `delete-session`, and
  with `-s` only at the top level (`zellij -s work`, `zellij -s work action ...`).
- `tb` has `-s` on every command, which clashes with `list-sessions -s` (`--short`). `tb` has to give `-s` to
  each subcommand that takes a session instead of declaring it once for all of them.
- Every session command has a short alias (`a`, `k`, `d`, `ka`, `da`, `ls`, `ac`).
- Not checked: zellij's exact behaviour when names clash, and what `attach` with no name does with several
  sessions running. The plan doesn't depend on them, because `tb` keeps its own forgiving behaviour there.
