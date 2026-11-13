# Plan: a GitHub Pages site for termos, featuring the one-line install

**Status:** in progress · **Date:** 2026-11-13 · **Asked by:** the user ("create a github page, feature the one
liner download") · **Part of:** [the marketing plan](2026-11-13-marketing.md) (layer 3, "stay findable")

## Decision

**One page, `site/index.html`, published by a GitHub Actions workflow to `https://hugegene.github.io/termos/`.**
The install line sits at the top, with a copy button. Below it come the 33-second demo GIF, what it replaces, and
links to the repo.

```
 main branch                          GitHub Actions (.github/workflows/pages.yml)         GitHub Pages
┌──────────────────────────┐  1 push  ┌────────────────────────────────────────────┐ 3 ┌──────────────────────────┐
│ site/index.html          │ ───────▶ │ checkout; copy site/ and two images into   │ ─▶│ hugegene.github.io/termos│
│ site/robots.txt, sitemap │          │ _site/; 2 upload it as the Pages artifact;  │   │  index.html, GIF, PNG    │
│ docs/images/demo-short.gif│         │ deploy                                      │   └──────────────────────────┘
│ docs/images/social-preview.png│     └────────────────────────────────────────────┘
└──────────────────────────┘
```

**What the page holds, top to bottom:**
- **Hero:** "termos", "a terminal OS", "Drop all desktop apps, use tos.", the install line with **Copy**, and the
  systems it runs on.
- **The demo:** the 33-second GIF from the README.
- **Quick start:** `tos`, then `Ctrl+t` `f` for the finder, and `Ctrl+s` / `Ctrl+e` in the editor.
- **Instead of…:** the README's table, shortened.
- **Three features:** sessions that come back after a reboot; a terminal file finder with previews; AI agents side
  by side, with prbot.
- **Links:** GitHub, the README, the command docs, releases, prbot. A footer with the MIT licence.

**For search engines:**
- the `<title>` "termos: a terminal OS and terminal workspace", and a meta description with all three phrases
- Open Graph tags, with the social-preview image
- a canonical URL, a `sitemap.xml` and a `robots.txt`
- schema.org `SoftwareApplication` data

The site is also set as the repo's homepage.

## Options considered

- **Pages from `/docs` on `main`:** no workflow needed, but it would publish all of `docs/` (plans, findings, CLI
  pages) as a Jekyll site, with the page buried among them.
- **A `gh-pages` branch:** a second history to keep in step, and the images copied into it by hand.
- **A workflow deploying `site/` (chosen):** the page lives next to the code. The GIF and image are copied from
  `docs/images/` at deploy time, so they're never stored twice. Pushes that don't touch them don't redeploy. On a
  public repo, Pages and Actions are free.
- **A static-site generator** (Hugo, Zola): too much for one page. It's plain HTML and CSS, with a few lines of JS
  for the copy button and no outside requests.

## Steps

1. `site/index.html`, `site/robots.txt`, `site/sitemap.xml`. Check them in headless Chrome at desktop and phone
   widths.
2. `.github/workflows/pages.yml`: on pushes to `main` touching `site/**`, the two images or itself, and by hand.
3. Turn on Pages with the "GitHub Actions" source, as `hugegene`:
   `gh api -X POST repos/hugegene/termos/pages -f build_type=workflow`.
4. Commit, push, and watch the deploy. Fetch the live page and check its title, its `og:image` and the GIF.
5. Set the repo's homepage to the site (`gh repo edit --homepage`), which shows beside the description.
6. *(user, optional)* Add the site to Google Search Console and submit `sitemap.xml`.
7. `CLAUDE.md`: add `site/` to the layout.
