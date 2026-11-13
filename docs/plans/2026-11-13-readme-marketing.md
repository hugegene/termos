# Plan: make the README sell termos

**Status:** done, except the social preview, which the user uploads · **Date:** 2026-11-13 · **Asked by:** the user ("do all you suggested") ·
**Builds on:** [the demo animation](2026-11-13-finder-demo-gif.md) · **Finding:**
[the short GIF, badges and repo settings](../findings/2026-11-13-readme-marketing.md)

## Decision

**The first screen of the README answers four questions in order:** what it is, what it looks like, how to
install it, and why to switch. Everything a visitor needs only once they're interested moves lower or folds away.

```
 first screen (before any scrolling)       lower down (unchanged content, reordered)
┌───────────────────────────────────────┐ ┌──────────────────────────────────────────────┐
│ # termos + badges                     │ │ Why termos? · Instead of… table              │
│ Drop all desktop apps, use `tos`.     │ │ AI agents side by side (Claude, prbot)       │
│ one-sentence pitch                    │ │ Sessions (diagram moves here) · Keys · Finder│
│ short demo GIF (28 s)                 │ │ the full tour GIF (~93 s), folded            │
│ curl … | sh  +  three-command start   │ │ Install details, folded · Extending · Dev    │
└───────────────────────────────────────┘ └──────────────────────────────────────────────┘
```

| # | Change | Where |
|---|---|---|
| 1 | Tagline, one-sentence pitch, short GIF, one-line install and a quick start come first. The architecture diagram moves under **Sessions** | top |
| 2 | An **Instead of…** table backs the tagline, honestly: spreadsheets are "for a quick look", as `tos` can't save xlsx | after the quick start |
| 3 | **AI agents side by side:** Claude Code in a tab, its files in the finder, prbot one per PR | near the top |
| 4 | A **short GIF** (28 s, 0.7 MB) of highlights cut from the same recording as the full tour. The full tour folds away in `<details>` | top, and **Finder** |
| 5 | Install details (PATH, SHA-256, versions, optional programs) fold into `<details>` | **Install** |
| 6 | Badges (release, licence, platforms); the GitHub repo's description and topics; a social preview image (uploaded by hand: GitHub has no API for it) | top, GitHub |
| 7 | "termos (`tos`)" named once; jargon (OSC 52, image protocols) stays lower; a **Why termos?** paragraph; no claims that tmux or zellij can't restore sessions (tmux-resurrect and zellij's resurrection can) | throughout |

## Example: a visitor's first 30 seconds

1. **They open github.com/hugegene/termos.** The badges show the latest release, MIT and Linux · macOS.
2. **The tagline:** "Drop all desktop apps, use `tos`."
3. **The pitch:** a terminal workspace that does the jobs of a file manager, viewers and tmux.
4. **The short GIF:** it flicks through a spreadsheet, a web page edited in Neovim, a video, a PDF page and a
   picture, then Claude writing a server in a new tab. It loops in 28 s, so they see all of it.
5. **The one-line install** and `tos`. They can try it without scrolling further.
6. **Scrolling on:** the "Instead of…" table says what it replaces, and what it doesn't.

## Options considered

- **One long GIF at the top (before):** 93 s. Most visitors see the first few steps, and a GIF can't be paused.
- **Three short GIFs (finder, sessions, Claude):** more to record and keep current. Sessions aren't visual
  enough to need their own GIF.
- **One short highlights GIF plus the full tour folded (chosen):** both come from one recording. The script
  marks the highlights as it goes, so a re-recording keeps them in step with the tour.

## Steps

1. `scripts/demo/record.py`:
   - The tour marks highlight stretches: a few seconds of each preview, and the edits and Claude's work sped up.
   - `--short-out` (default `docs/images/demo-short.gif`) cuts those from the same captioned recording.
2. Record again with the user's media (the command in [the demo plan](2026-11-13-finder-demo-gif.md)).
   Measure the short GIF's length and size, in a finding.
3. Rewrite the README as above.
4. A social preview image, 1280×640, from a demo frame: `docs/images/social-preview.png`.
5. Set the description and topics as `hugegene`, with
   `GH_TOKEN=$(gh auth token --user hugegene) gh repo edit …`. The active `gh` account is the work one; don't
   switch it.
6. The user uploads the social preview in Settings → General → Social preview.

## Changed: found by "terminal OS", "terminal workspace" and "terminal file finder" (2026-11-13)

**Asked by the user:** people searching for these three phrases should find the repo. **Baseline:** termos was in
none of GitHub's top 100 for any of them, nor for "termos", which is Portuguese for "terms"
([finding](../findings/2026-11-13-search-baseline.md)).

**Done:** each phrase is placed once, where search engines read it, with no stuffing.
- **The repo's description (GitHub, as `hugegene`):** "termos: a terminal OS and terminal workspace. Sessions,
  tabs and splits, with a terminal file finder that previews and edits PDFs, images, videos, Office files and
  CSV. Drop all desktop apps." GitHub search matches it, and it's Google's page title.
- **Topics:** added `terminal-os`, `terminal-workspace`, `file-finder`, `terminal-file-manager`, `productivity`,
  `developer-tools`, `ai-agents` and `workspace`, which makes 18.
- **README:**
  - the heading is `# termos: a terminal OS`
  - the pitch says "short for terminal OS, is a terminal workspace with a terminal file finder built in"
  - `## Finder` is now `## Terminal file finder`
- **Releases:** `release.yml` titles them "termos X: a terminal OS". v0.4.3 was retitled to match and given notes.
- **Social preview:** `docs/images/social-preview.png` now has "a terminal OS". The user uploads it by hand.

**Not done:** launching and the rest now live in [the marketing plan](2026-11-13-marketing.md). Kept below as
written on the day:
- **Stars and links:** a Show HN post, Reddit (r/commandline, r/rust, r/linux), lobste.rs, awesome lists
  (awesome-tuis, awesome-cli-apps, awesome-rust, awesome-claude-code), and a Homebrew tap and an AUR package.
- **Optional:** a GitHub Pages site set as the homepage.

**To check:** re-run the baseline's searches in a day or two, and again after any launch, and record a new
finding. **Estimate:** "terminal file finder" should reach the first page at once; "terminal OS" needs some
stars (the leaders have 39–276); "terminal workspace" is led by zellij, with 36 thousand.
