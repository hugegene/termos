# Finding: where termos shows up in GitHub search (2026-11-13)

## Question

The user wants people searching for "terminal os" and "terminal workspace" to find the repo. Where does it rank
now, and what does it compete with? Which changes are worth making depends on it.

## Method

`gh` (as `hugegene`), the repo at v0.4.3, with 1 star and no homepage set:

```
gh search repos "<query>" --limit 100 --json fullName,stargazersCount,description
```

The queries were "terminal os", "terminal workspace" and "termos", sorted by GitHub's "best match". For each, the
top 6 results were noted, and the 100 were searched for `hugegene/termos`.

## Results

```
"terminal os"         1 MenkeTechnologies/zpwr 227★ "ZPWR => Terminal OS"
                      2 nilseuropa/solar_os 276★ "…a small text-first pocket terminal OS…"
                      3 terroo/terminalroot-os 39★   4 rolandnsharp/terminal-oscilloscope 109★
                      5 wbopan/vscode-terminal-osc-notifier 11★   6 HydeHunter2/terminal-OS 22★
                      hugegene/termos: not in the top 100
"terminal workspace"  1 zellij-org/zellij 36047★ "A terminal workspace with batteries included"
                      2 nyakang/nyaterm 2396★   3 with-hide/hide 102★   4 taixu-lab/taixu 188★
                      5 stumptowndoug/shep 86★ "A native terminal workspace for your coding agents"
                      6 xuzhougeng/wispterm 470★
                      hugegene/termos: not in the top 100
"termos"              1–6: Portuguese repos ("termos" means "terms"), 23–715★
                      hugegene/termos: not in the top 100
```

The repo's description then was "Drop all desktop apps, use tos: a terminal workspace with sessions, tabs,
splits and a file finder that previews and edits files". Its topics were claude-code, cli, file-manager,
file-preview, rust, terminal, terminal-multiplexer, tmux, tui and zellij.

## Conclusions

- **termos isn't in the top 100 for any of the three queries,** not even its own name.
- **The leaders all have the exact phrase in their description,** and they have stars. "terminal os" is won with
  only 39–276 stars, so it's reachable. "terminal workspace" is led by zellij (36 thousand stars), so the realistic
  aim there is the first page, not the top.
- **"termos" alone collides with Portuguese "terms",** so the name should always be paired with "terminal OS"
  or `tos`.
- **Unknown:** how much each lever moves the rank. GitHub doesn't publish its ranking. Re-run this search after
  changing the description, the topics and the README, and again after any launch, and add a new dated finding.

## Added later the same day: "terminal file finder"

**Method:** the same `gh search repos` (100 results asked for). Then the sizes of two topics, from
`gh api "search/repositories?q=topic:<topic>&per_page=1" --jq .total_count`.

**Results:**

```
"terminal file finder"   3 results in all:
                         1 hanilr/tff 7★ "Terminal File Finder Written In C++"
                         2 hamonikr/hamonikr-ff 0★ "hamonikr terminal file finder"
                         3 hariharen9/wtf 0★ "…interactive terminal file finder and CLI s…"
                         hugegene/termos: not in the results
topic:terminal-file-manager   24 repos
topic:file-finder             50 repos
```

**Conclusions:**
- **"terminal file finder" has almost no competition.** With the exact phrase in termos's description, it
  should reach the first page (an **estimate**, to check by re-running the search).
- **The two topics are small,** so termos would be easy to find within them.
- **The nearby query "terminal file manager" wasn't measured.** Large projects are known to own it (yazi,
  ranger, lf, nnn).
