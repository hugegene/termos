# termos

A terminal workspace: persistent sessions, tabs and split panes, with a file finder and previews built in.
The command is `tos`. Source and docs: https://github.com/hugegene/termos

## Install

Put `tos` in a folder on your `PATH`, such as `~/.local/bin`. Or use the installer, which does the same,
checks the download, and adds `~/.local/bin` to your shell's `PATH` if it's missing (`TOS_NO_MODIFY_PATH=1`
skips that):

    curl -fsSL https://github.com/hugegene/termos/releases/latest/download/install.sh | sh

Then run `tos` to start or attach to the session "main".

## Optional tools

Some previews use other programs if they're installed. When one is missing, `tos` names the package:

- Neovim: editing files
- poppler: PDF previews
- ffmpeg: video previews
- gio or trash: deleting to the trash

## License

MIT, see `LICENSE`. The licenses of the libraries built into `tos` are in `THIRD_PARTY_NOTICES.md`.
