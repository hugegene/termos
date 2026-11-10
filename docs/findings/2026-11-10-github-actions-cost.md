# GitHub Actions cost, and whether the logged-in account can host the repos

## Question

Does GitHub Actions cost money for the [distribution plan](../plans/2026-11-09-distribution.md)? The plan
has a private source repo, CI on every push (with a macOS job), release builds for three targets, and a public
releases repo. The answer closes the plan's open question about Actions minutes. It also decides which GitHub
account should own the repos.

## Method

On 2026-11-10, on Ubuntu 24.04.5 LTS (Linux 6.8.0-149, x86_64), with `gh version 2.45.0`:

- Read GitHub's billing page for Actions: <https://docs.github.com/en/billing/concepts/product-billing/github-actions>
- Read GitHub's releases page: <https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases>
- Read GitHub's page on managed user accounts:
  <https://docs.github.com/en/enterprise-cloud@latest/admin/managing-iam/understanding-iam-for-enterprises/abilities-and-restrictions-of-managed-user-accounts>
- Read the account `gh` is logged in as (read-only calls):

  ```sh
  gh auth status
  gh api user --jq '{login, type, plan: .plan}'
  gh api user/orgs --jq '.[].login'
  gh api user/repos --jq '.[] | "\(.full_name) \(.visibility)"'
  ```

- Read `.github/workflows/ci.yml` as of commit `4ade7b8`.

## Results

### Actions billing (quoted from the billing page)

- "Standard GitHub-hosted runners in public repositories" are listed as usage that "remains free
  independently and does not reduce the included compute usage."
- Included compute for private repos, per month. "The included compute usage is expressed as a monetary amount,
  not a fixed number of minutes":

  | Plan | Included compute | Artifact storage |
  |---|---|---|
  | GitHub Free | $12 | 500 MB |
  | GitHub Pro | $18 | 1 GB |
  | GitHub Team | $18 | 2 GB |
  | GitHub Enterprise Cloud | $300 | 50 GB |

- Per-minute rates:

  | Runner | SKU | Per minute |
  |---|---|---|
  | Linux 2-core x64 (`ubuntu-latest`) | `actions_linux` | $0.006 |
  | Linux 2-core arm64 | `actions_linux_arm` | $0.005 |
  | Windows 2-core | `actions_windows` | $0.010 |
  | macOS 3- or 4-core, M1 or Intel (`macos-14`) | `actions_macos` | $0.062 |

- "If your account does not have a valid payment method on file, usage is blocked once you consume the
  included compute usage."
- Costs are "billed to the repository owner", not to the person who triggered the run.

### Releases (quoted from the releases page)

- "Each file included in a release must be under 2 GiB. There is no limit on the total size of a release, nor
  bandwidth usage."
- The page doesn't say whether release files count against storage.

### The logged-in account (work login and org names redacted: the repo is public)

```
$ gh auth status
github.com
  ✓ Logged in to github.com account <work-login>_<SHORTCODE> (keyring)
$ gh api user --jq '{login, type, plan: .plan}'
{"login":"<work-login>_<SHORTCODE>","plan":null,"type":"User"}
$ gh api user/orgs --jq '.[].login'
(no output)
$ gh api user/repos --jq '.[] | "\(.full_name) \(.visibility)"'
<employer-org>/<repo> private
```

From the managed-user page:

- Managed users "can contribute only to private and internal repositories within their enterprise."
- Whether they can create their own repos depends on an enterprise policy. If they can, "they can only own
  private repositories."
- GitHub-hosted runners "are not available for repositories owned by managed user accounts."

### What CI runs today (`ci.yml`)

It runs on every push and every pull request, with three jobs: `check` (`ubuntu-latest`: fmt, two clippy
runs, `cargo test`), `macos` (`macos-14`: `brew install poppler`, `cargo test`) and `licenses`
(`ubuntu-latest`: `cargo-deny`). There's no build cache, so every job compiles all dependencies from scratch.

## Conclusions

- **Public repos cost nothing.** The `tb-releases` repo and its downloads are free. The releases page sets no
  bandwidth limit.
- **The private source repo spends money once it's over the free amount.** On GitHub Free, $12 a month buys
  about 2,000 Linux minutes or about 190 macOS minutes. A macOS minute costs about 10 times a Linux minute.
- **Each push costs about $0.69 (estimate).** This assumes each job takes about 10 minutes from a cold build,
  which hasn't been measured. That's about $0.62 for `macos`, $0.06 for `check` and under $0.01 for `licenses`.
  At that rate, $12 covers about 17 pushes a month. The macOS job uses most of it.
- **Each release costs about $0.73 (estimate).** That's x86_64 Linux at about $0.06, arm64 Linux at about $0.05
  and macOS at about $0.62.
- **No surprise bill without a card.** With no payment method on file, runs stop once the $12 is used and start
  again next month. CI is then off for the rest of the month.
- **Ways to spend less** (not decided):
  - Add a Rust build cache to CI.
  - Run the `macos` job only on `main`, on tags or by hand, not on every push.
  - Run the release builds in the public `tb-releases` repo, where runners are free. That would make the build
    logs public.
- **The logged-in account looks like a work account and can't host this plan.** The login ends in `_<SHORTCODE>`. That
  matches the `<name>_<shortcode>` format of Enterprise Managed User accounts, but this is from memory: the page
  above doesn't state the format. Its only visible repo belongs to the employer's org. If it's a managed account, it can't
  create the public `tb-releases` repo, it can't use GitHub-hosted runners on its own repos, and anything it
  owns sits inside the employer's enterprise. The repos should be on a personal GitHub account.
- **Still unknown:**
  - Real job times. The first CI run gives them; record them in a new finding.
  - Whether `<work-login>_<SHORTCODE>` is a managed work account (ask the user).
  - Which plan the personal account is on.
