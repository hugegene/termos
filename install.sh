#!/usr/bin/env bash
# One-time setup for the terminal finder. Safe to re-run.
set -euo pipefail
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
mkdir -p "$HOME/.local/bin" "$HOME/.local/opt"

echo "==> apt: chafa (images), bat (text), visidata (csv editing), python3-html2text + w3m (html)"
need=()
for pkg in chafa bat visidata python3-html2text w3m; do dpkg -s "$pkg" >/dev/null 2>&1 || need+=("$pkg"); done
if [ ${#need[@]} -gt 0 ]; then sudo apt-get install -y "${need[@]}"; else echo "    already installed"; fi

echo "==> Neovim (latest release) → ~/.local/opt, linked as ~/.local/bin/nvim"
if "$HOME/.local/bin/nvim" --headless +'lua if vim.fn.has("nvim-0.10") == 0 then os.exit(1) end' +q 2>/dev/null; then
  echo "    already installed: $("$HOME/.local/bin/nvim" --version | head -1)"
else
  tmp="$(mktemp -d)"
  curl -fL --progress-bar -o "$tmp/nvim.tar.gz" \
    https://github.com/neovim/neovim/releases/latest/download/nvim-linux-x86_64.tar.gz
  rm -rf "$HOME/.local/opt/nvim-linux-x86_64"
  tar -xzf "$tmp/nvim.tar.gz" -C "$HOME/.local/opt"
  ln -sfn "$HOME/.local/opt/nvim-linux-x86_64/bin/nvim" "$HOME/.local/bin/nvim"
  rm -rf "$tmp"
  echo "    $("$HOME/.local/bin/nvim" --version | head -1)"
fi

echo "==> render-markdown.nvim (Neovim plugin, finder-only)"
plug="$HOME/.local/share/tfinder-nvim/site/pack/tfinder/start/render-markdown.nvim"
if [ -d "$plug/.git" ]; then
  git -C "$plug" pull -q --ff-only && echo "    updated"
else
  mkdir -p "$(dirname "$plug")"
  git clone -q --depth 1 https://github.com/MeanderingProgrammer/render-markdown.nvim "$plug" && echo "    installed"
fi

echo "==> 'finder' command → ~/.local/bin/finder"
chmod +x "$HERE/finder" "$HERE/bin/files" "$HERE/bin/send" "$HERE/preview.py"
ln -sfn "$HERE/finder" "$HOME/.local/bin/finder"

echo
echo "Done. Run:  finder [folder]"
