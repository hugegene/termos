# Plan: get termos found and used

**Status:** started: the words are done, launching hasn't started · **Date:** 2026-11-13 · **Asked by:** the
user ("put all the marketing ideas in a plan") · **Finding:**
[where termos shows up in GitHub search](../findings/2026-11-13-search-baseline.md) · **Builds on:**
[the README plan](2026-11-13-readme-marketing.md), [the demo animation](2026-11-13-finder-demo-gif.md)

## Decision

**Three layers, in this order:** be findable, launch, stay findable. Measure after each.

```
 1 BE FINDABLE (done)        2 LAUNCH (bursts)                 3 STAY FINDABLE (lasting links)
┌──────────────────────┐   ┌────────────────────────────┐    ┌───────────────────────────────┐
│ README, description, │   │ Show HN                    │    │ awesome lists (pull requests) │
│ topics, release      │──▶│ Reddit: commandline, rust  │──▶ │ Homebrew tap, AUR             │
│ titles, demo GIFs,   │   │ lobste.rs, X, Bluesky      │    │ a GitHub Pages site           │
│ social image         │   │ (one a day)                │    │ terminal.guide                │
└──────────────────────┘   └────────────┬───────────────┘    └───────────────┬───────────────┘
                                        └──── visitors ─▶ stars + links ◀─────┘
                                                              │
                                                              ▼
                             higher for "terminal OS", "terminal workspace", "terminal file finder"
                             (4 MEASURE: re-run the search baseline, record a new finding)
```

**Why this order:**
- **Words first:** a visitor sent by a launch should land on a page that explains itself in seconds. That's the
  short GIF, the "Instead of…" table and the one-line install. Done.
- **Then launches:** stars are what GitHub search ranks by. The leaders for "terminal OS" have only 39–276 stars,
  so a launch or two can win that phrase. "Terminal file finder" needs only the words (3 competing repos).
  "Terminal workspace" is led by zellij (36 thousand stars), so its first page is the realistic goal
  ([finding](../findings/2026-11-13-search-baseline.md)).
- **Lasting links last:** some awesome lists only accept projects with some stars, and package managers matter
  once people want to install.

**Who does what:**
- **The user:** anything posted under the user's name or account. That's every post, every pull request to someone
  else's list, the AUR, and the social-preview upload.
- **Claude:** drafts and anything in this repo, and, with the user's go-ahead, new repos under `hugegene` (a
  Homebrew tap).

## Example: a launch week

1. **Monday:**
   - The user uploads `docs/images/social-preview.png` (Settings → General → Social preview).
   - Claude checks that GitHub's `og:image` now points to it.
   - The user sends the repo link to Telegram's @WebpageBot, so earlier previews refresh.
2. **Tuesday, US morning:** the user posts "Show HN: Termos – a terminal OS with a built-in file finder and
   previews", adds the first comment, and answers comments for a few hours.
3. **Wednesday:** r/commandline, with the 28-second GIF.
4. **Thursday:** r/rust, from the angle of how it's built (a fork of `alacritty_terminal`, a client and a
   server).
5. **Friday:**
   - The user sends a pull request to awesome-tuis and one to awesome-cli-apps.
   - Claude re-runs the search baseline and writes a new finding: where termos now ranks for the three phrases.

The times are an **estimate**, from common advice on posting to HN: weekday mornings in the US get the most
readers.

## All the ideas

**Done:**

| Idea | Where |
|---|---|
| README: tagline, pitch, 28-second GIF and install first; "Instead of…"; "Why termos?"; "AI agents side by side"; install details folded | [README plan](2026-11-13-readme-marketing.md), `ab4265b` |
| Badges: release, licence, platforms, Rust | README |
| Demo GIFs: short (28 s) and the full tour (93 s), with the user's Ghibli media | [demo plan](2026-11-13-finder-demo-gif.md) |
| prbot linked, with its GIF embedded, under Extending | README |
| The three phrases in the description, the topics (18), the README heading, pitch and "Terminal file finder" heading | `70438f7`, and GitHub's settings |
| Release titles "termos X: a terminal OS"; v0.4.3 retitled, with notes | `release.yml`, GitHub |
| Social preview image, 1280×640, "termos: a terminal OS" | `docs/images/social-preview.png` |

**The user's part:**

| Idea | Why | Notes |
|---|---|---|
| **Upload the social preview** | Links in Slack, X, LinkedIn, Discord, WhatsApp and Telegram show it instead of GitHub's automatic card | Settings → General → Social preview. Refresh old previews with Telegram's @WebpageBot, LinkedIn's Post Inspector and Facebook's Sharing Debugger |
| **Show HN** | The biggest single burst of programmers | Title (71 characters) above. The first comment is in your own voice: what it is, why, what's unusual (a finder that previews PDFs, images, video and Office files inside a multiplexer; sessions that survive reboots; zellij's keys and CLI), and what's missing (floating panes, plugins, saving Office files). No asking for votes; stay to answer |
| **Reddit** | Bursts from the right readers | r/commandline (the GIF); r/rust (how it's built); r/ClaudeAI or similar (the AI-agent angle, with prbot). One a day; read each subreddit's rules on self-promotion first |
| **lobste.rs** | A smaller, technical audience | Needs an invitation. Tags `show`, `rust`, `cli` |
| **X, Bluesky, Mastodon, LinkedIn** | Reach through your own network | The 28-second GIF, uploaded as video, with one line and the link |
| **Awesome lists** (pull requests) | Lasting links that people browse for years | awesome-tuis (Multiplexers or File Managers), awesome-cli-apps (Files and Directories), awesome-rust (Applications; check its `CONTRIBUTING.md`, which may ask for a minimum of stars or downloads), awesome-claude-code (prbot fits best). One line each, in the list's own style |
| **terminal.guide** | A directory of terminal tools that people search | Submit termos as a terminal workspace |
| **Pin the repo on your GitHub profile** | Visitors to your profile see it first | With prbot beside it |
| **An AUR package** | Arch users install from the AUR, and it gives a listing page | Needs an AUR account; Claude can write the `PKGBUILD` |
| **Google Search Console** | Asks Google to index the site sooner | Once there's a GitHub Pages site (below) |
| *Optional:* **Product Hunt** | A broader, less technical audience | Less suited to a terminal tool; after the rest |

**Claude's part** (on request; new repos under `hugegene` only with the user's go-ahead):

| Idea | Why | Notes |
|---|---|---|
| **Draft the posts and entries** | You edit and post them, in your own voice | The Show HN first comment, each Reddit post, and the awesome-list lines in each list's format |
| **A Homebrew tap** | `brew install hugegene/tap/termos`, the way Mac users expect | A new repo `hugegene/homebrew-tap`, with a formula pointing at the release's macOS (and Linux) tarballs and their SHA-256. Each release then needs its version and hashes updated, which `release.yml` could do |
| **A GitHub Pages site** | A second page for Google, with the title "termos: a terminal OS and terminal workspace" | One page: the pitch, the GIF, the install line and links. Set it as the repo's homepage |
| **Re-run the search baseline** | Shows what worked | After the words (in a day or two), after each launch, and a month on; a new dated finding each time |
| **Release notes with highlights** | Release pages are indexed and read before upgrading | A few lines on what's new in each release, as for v0.4.3 |
| **Keep the demo current** | A stale GIF misleads | `scripts/demo/record.py` after finder or key changes (the command is in `CLAUDE.md`) |
| **Invite contributors** | Activity and contributors help rank and trust | Turn on GitHub Discussions, and label a few issues `good first issue` |

## Options considered and not chosen

- **Repeating the phrases in the README:** GitHub and Google treat keyword stuffing as spam. Each phrase
  appears once where it's read.
- **Renaming to avoid "termos" (Portuguese for "terms"):** the name is established. Pairing it with "terminal OS"
  or `tos` everywhere is enough.
- **crates.io (`cargo install`):** `tos` depends on our fork of `alacritty_terminal` by path. That fork would have
  to be published too, under a new name, and "termos" and "tos" may already be taken (an **estimate**, not
  checked). Worth checking later; not first.
- **Paid promotion, or buying stars:** paying for stars breaks GitHub's terms, and fake stars don't bring users.

## Steps

1. *(user)* Upload the social preview. *(Claude)* Check `og:image`, and refresh Telegram's preview through
   @WebpageBot.
2. *(Claude, on request)* Draft the Show HN comment, the Reddit posts and the awesome-list lines.
3. *(user)* Show HN, then one Reddit post a day, as in the example.
4. *(Claude)* After two days, re-run the search baseline; a new finding.
5. *(user)* Awesome-list pull requests and terminal.guide; pin the repo.
6. *(Claude, with the user's go-ahead)* The Homebrew tap; then the Pages site, and *(user)* Search Console.
7. *(Claude)* A month on, re-run the search baseline again; a new finding. Then decide on the AUR and crates.io.
