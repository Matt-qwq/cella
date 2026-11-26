//! `cargo xtask docs`: every relative link in the repository's Markdown resolves.
//!
//! Documentation rots silently: a page renamed, a section retitled, an image moved, and the link
//! that pointed at it is only discovered by whoever clicks it next. This step opens every link the
//! docs write — `docs/**/*.md` and the pages at the repository root — and fails on one that does
//! not resolve.
//!
//! An **anchor** is checked too (`page.md#beta-gates`), including a bare `#anchor` that points into
//! the page it is written on: the target's headings are turned into GitHub's slugs (lower case,
//! punctuation dropped, spaces to dashes, repeats numbered), so a renamed section fails here instead
//! of sending a reader to the top of the page. Links to `http(s)`, `mailto:` and the web at large
//! are left alone: this check is about the files in this repository, not about the internet being up.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The fence a Markdown block was opened with.
const BACKTICKS: &str = "```";
const TILDES: &str = "~~~";

/// Every Markdown file this step follows links in: `docs/**` and the pages at the root.
fn markdown_files(root: &Path) -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "--cached", "--others", "--exclude-standard", "*.md"])
        .output()
        .map_err(|e| format!("git ls-files: {e}"))?;
    if !out.status.success() {
        return Err(format!("git ls-files failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let files: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.ends_with(".md"))
        .filter(|l| l.starts_with("docs/") || !l.contains('/'))
        .filter(|l| root.join(l).exists())
        .map(str::to_owned)
        .collect();
    Ok(files)
}

/// The fence a line opens, if it opens one.
fn opens_fence(line: &str) -> Option<&'static str> {
    if line.starts_with(BACKTICKS) {
        Some(BACKTICKS)
    } else if line.starts_with(TILDES) {
        Some(TILDES)
    } else {
        None
    }
}

/// Whether `line` closes the fence opened with `marker`.
fn closes_fence(line: &str, marker: &str) -> bool {
    line.starts_with(marker)
}

/// The relative link targets in `text`, with the line each is on, skipping fenced code blocks
/// (a link written there is an example, not a link) and anything that leaves the repository.
fn links_in(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut fence: Option<&'static str> = None;
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        match fence {
            Some(marker) if closes_fence(trimmed, marker) => {
                fence = None;
                continue;
            }
            Some(_) => continue,
            None => {
                if let Some(marker) = opens_fence(trimmed) {
                    fence = Some(marker);
                    continue;
                }
            }
        }
        let mut rest = line;
        while let Some(at) = rest.find("](") {
            let after = &rest[at + 2..];
            let Some(end) = matching_paren(after) else { break };
            let target = after.get(..end).unwrap_or_default().trim();
            if let Some(link) = relative_target(target) {
                out.push((i + 1, link));
            }
            rest = after.get(end + 1..).unwrap_or_default();
        }
    }
    out
}

/// The index of the `)` that closes the one just before `text`, counting nesting (a link's target
/// may hold parentheses, as a heading on GitHub can: `page.md#a-(b)`).
fn matching_paren(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(i),
            ')' => depth -= 1,
            '\n' => return None,
            _ => {}
        }
    }
    None
}

/// The target as a repository path with its anchor, or `None` when it is not one of ours
/// (an absolute URL or a mailto).
fn relative_target(raw: &str) -> Option<String> {
    let raw = raw.strip_prefix('<').and_then(|r| r.strip_suffix('>')).unwrap_or(raw).trim();
    if raw.is_empty() || raw.starts_with("mailto:") || raw.contains("://") {
        return None;
    }
    // A target written with a title ("(x.png "the shot")") keeps only its path.
    let raw = raw.split_once('"').map_or(raw, |(path, _)| path).trim();
    Some(raw.replace("%20", " "))
}

/// GitHub's slug for a heading: lower case, everything that is not a letter, a digit, a space, `-`
/// or `_` dropped, spaces turned into dashes.
fn slug(heading: &str) -> String {
    let mut out = String::with_capacity(heading.len());
    for c in heading.trim().chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            out.extend(c.to_lowercase());
        } else if c.is_whitespace() {
            out.push('-');
        }
    }
    out
}

/// The anchors a page offers, with GitHub's numbering for a heading that appears twice (`-1`, `-2`).
fn anchors(text: &str) -> BTreeSet<String> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = BTreeSet::new();
    let mut fence: Option<&'static str> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        match fence {
            Some(marker) if closes_fence(trimmed, marker) => {
                fence = None;
                continue;
            }
            Some(_) => continue,
            None => {
                if let Some(marker) = opens_fence(trimmed) {
                    fence = Some(marker);
                    continue;
                }
            }
        }
        let Some(heading) = trimmed.strip_prefix('#') else { continue };
        let heading = heading.trim_start_matches('#').trim();
        if heading.is_empty() {
            continue;
        }
        let base = slug(heading);
        let n = seen.entry(base.clone()).or_insert(0);
        out.insert(if *n == 0 { base.clone() } else { format!("{base}-{n}") });
        *n += 1;
    }
    out
}

/// Every link in `files` that does not resolve, as `file:line: target — why`.
pub fn missing(root: &Path, files: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut anchor_cache: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(root.join(file)) else { continue };
        let dir = Path::new(file).parent().map(Path::to_path_buf).unwrap_or_default();
        for (line, target) in links_in(&text) {
            let (path, anchor) = match target.split_once('#') {
                Some((path, anchor)) => (path, Some(anchor)),
                None => (target.as_str(), None),
            };
            let target_path = if path.is_empty() { root.join(file) } else { root.join(normalise(&dir.join(path))) };
            if !target_path.exists() {
                let shown = if path.is_empty() { target.as_str() } else { path };
                out.push(format!("{file}:{line}: {shown} (no such file)"));
                continue;
            }
            let Some(anchor) = anchor.filter(|a| !a.is_empty()) else { continue };
            let anchors = anchor_cache
                .entry(target_path.clone())
                .or_insert_with(|| std::fs::read_to_string(&target_path).map(|t| anchors(&t)).unwrap_or_default());
            if !anchors.contains(anchor) {
                out.push(format!("{file}:{line}: {target} (no such heading)"));
            }
        }
    }
    out
}

/// `docs/./a/../b.md` → `docs/b.md`, so a path can be resolved without asking the file system.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

pub fn run(root: &Path) -> Result<(), String> {
    let files = markdown_files(root)?;
    let bad = missing(root, &files);
    if bad.is_empty() {
        println!("docs: {} Markdown file(s), every relative link and anchor resolves", files.len());
        Ok(())
    } else {
        Err(format!("{} link(s) do not resolve:\n  {}", bad.len(), bad.join("\n  ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Markdown file whose links are written with the three-backtick fence where the test needs
    /// one, so the test source does not have to hold a fence inside a string literal.
    fn fenced(inner: &str) -> String {
        format!("{BACKTICKS}\n{inner}\n{BACKTICKS}\n")
    }

    #[test]
    fn slugs_follow_githubs_rule() {
        // Both of these are real anchors the repository links to.
        assert_eq!(slug("M19 — Cold paging — 60–90 h"), "m19--cold-paging--6090-h");
        assert_eq!(slug("Measured against Excel and WPS (2026-11-18)"), "measured-against-excel-and-wps-2026-11-18");
        assert_eq!(slug("Beta gates"), "beta-gates");
        assert_eq!(slug("  数据模型  "), "数据模型");
    }

    #[test]
    fn anchors_number_a_repeated_heading() {
        let set = anchors("# Beta gates\n\ntext\n\n## Beta gates\n\nmore\n");
        assert!(set.contains("beta-gates") && set.contains("beta-gates-1"), "{set:?}");
    }

    #[test]
    fn links_skip_code_and_the_world_outside() {
        let text = format!(
            "1. a [page](rendering.md) and an [anchor](gaps.md#74-a-title)\n\
             2. ![shot](images/hero.png) and [web](https://example.com) and [mail](mailto:a@b.c)\n\
             {}\
             3. [same page](#a-heading) and [titled](x.png \"the shot\")\n",
            fenced("[a link in an example](does-not-exist.md)")
        );
        let got: Vec<(usize, String)> = links_in(&text).into_iter().collect();
        assert_eq!(
            got,
            vec![
                (1, "rendering.md".to_string()),
                (1, "gaps.md#74-a-title".into()),
                (2, "images/hero.png".into()),
                (6, "#a-heading".into()),
                (6, "x.png".into()),
            ],
            "{got:?}"
        );
    }

    #[test]
    fn a_broken_link_and_a_broken_anchor_are_reported() {
        let dir = std::env::temp_dir().join(format!("cella-docs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("docs/page.md"), "# A heading\n\n[good](other.md#a-heading)\n").unwrap();
        std::fs::write(
            dir.join("docs/other.md"),
            "# A heading\n\n[gone](missing.md)\n[wrong](page.md#no-such-heading)\n[nowhere](#missing-heading)\n",
        )
        .unwrap();
        let files = vec!["docs/page.md".to_string(), "docs/other.md".into()];
        let bad = missing(&dir, &files);
        assert_eq!(bad.len(), 3, "{bad:?}");
        assert!(bad[0].starts_with("docs/other.md:3: missing.md"), "{bad:?}");
        assert!(bad[1].contains("page.md#no-such-heading") && bad[1].contains("no such heading"), "{bad:?}");
        assert!(bad[2].contains("#missing-heading") && bad[2].contains("no such heading"), "{bad:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
