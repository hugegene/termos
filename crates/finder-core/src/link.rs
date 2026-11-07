//! Where a link in a previewed file points. Plan: docs/plans/2026-11-07-markdown-links.md.

use std::path::{Component, Path, PathBuf};

/// A link's target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A file or folder on this machine (it may not exist).
    Path(PathBuf),
    /// A heading in the same file (`#usage`).
    Heading(String),
    /// A link with a scheme other than `file:`, such as `https:` or `mailto:`.
    External(String),
}

/// Resolves `url`, a link found in a file in folder `base`. Returns `None` for an empty link.
///
/// Paths are relative to `base` unless absolute. `#…` and `?…` endings are dropped, `%xx` escapes decoded and
/// `.` and `..` folded without looking at the disk, as a browser does.
pub fn resolve(url: &str, base: &Path) -> Option<Target> {
    let url = url.trim();
    if let Some(heading) = url.strip_prefix('#') {
        return Some(Target::Heading(heading.to_owned()));
    }
    let path = match url.strip_prefix("file://") {
        // file:///abs/path, or file://localhost/abs/path
        Some(rest) => rest.strip_prefix("localhost").unwrap_or(rest),
        None if has_scheme(url) => return Some(Target::External(url.to_owned())),
        None => url,
    };
    let path = path.split(['#', '?']).next().unwrap_or_default();
    if path.is_empty() {
        return None;
    }
    let path = PathBuf::from(percent_decode(path));
    Some(Target::Path(normalize(&base.join(path))))
}

/// `https:`, `mailto:` and the like: letters, then letters, digits, `+`, `-` or `.`, then a colon.
fn has_scheme(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once(':') else { return false };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Decodes `%xx` escapes; anything that isn't a valid escape is kept as it is.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| char::from(b).to_digit(16);
        if bytes[i] == b'%'
            && let (Some(hi), Some(lo)) =
                (bytes.get(i + 1).and_then(|b| hex(*b)), bytes.get(i + 2).and_then(|b| hex(*b)))
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Folds `.` and `..` without following symlinks: `/a/b/../c` is `/a/c`.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(url: &str) -> PathBuf {
        match resolve(url, Path::new("/docs/plans")) {
            Some(Target::Path(p)) => p,
            other => panic!("{url}: {other:?}"),
        }
    }

    #[test]
    fn relative_paths() {
        assert_eq!(path("next.md"), Path::new("/docs/plans/next.md"));
        assert_eq!(path("./next.md"), Path::new("/docs/plans/next.md"));
        assert_eq!(path("../findings/x.md"), Path::new("/docs/findings/x.md"));
        assert_eq!(path("../../../../up.md"), Path::new("/up.md"));
        assert_eq!(path("sub/"), Path::new("/docs/plans/sub"));
    }

    #[test]
    fn absolute_paths_and_file_urls() {
        assert_eq!(path("/etc/hosts"), Path::new("/etc/hosts"));
        assert_eq!(path("file:///etc/hosts"), Path::new("/etc/hosts"));
        assert_eq!(path("file://localhost/etc/hosts"), Path::new("/etc/hosts"));
    }

    #[test]
    fn endings_and_escapes() {
        assert_eq!(path("other.md#usage"), Path::new("/docs/plans/other.md"));
        assert_eq!(path("other.md?plain=1"), Path::new("/docs/plans/other.md"));
        assert_eq!(path("my%20notes.md"), Path::new("/docs/plans/my notes.md"));
        assert_eq!(path("100%.md"), Path::new("/docs/plans/100%.md"));
        assert_eq!(path("caf%C3%A9.md"), Path::new("/docs/plans/café.md"));
    }

    #[test]
    fn headings_schemes_and_empty_links() {
        let base = Path::new("/docs");
        assert_eq!(resolve("#usage", base), Some(Target::Heading("usage".into())));
        assert_eq!(resolve("https://x.org/a.md", base), Some(Target::External("https://x.org/a.md".into())));
        assert_eq!(resolve("mailto:a@b.c", base), Some(Target::External("mailto:a@b.c".into())));
        assert_eq!(resolve("", base), None);
        assert_eq!(resolve("?x", base), None);
    }
}
