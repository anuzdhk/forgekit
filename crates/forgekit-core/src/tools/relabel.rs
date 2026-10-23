//! Relabel: bulk rename with find/replace, regex and numbering.
//!
//! `plan` computes every rename without touching the disk so callers can show
//! a live preview; `apply` performs a plan.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use regex::Regex;

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub find: String,
    pub replace: String,
    pub regex: bool,
    pub ignore_case: bool,
    /// Leave the extension alone.
    pub keep_extension: bool,
    /// Template applied after replacing. `{name}` is the (replaced) stem,
    /// `{ext}` the extension, `{n}` / `{n:3}` a counter (zero-padded to 3).
    pub template: Option<String>,
    pub start: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    pub from: PathBuf,
    pub to: PathBuf,
    /// Why this rename can't happen, if it can't.
    pub problem: Option<String>,
}

impl Rename {
    pub fn changes(&self) -> bool {
        self.from != self.to
    }
}

pub fn plan(files: &[PathBuf], opts: &Options) -> anyhow::Result<Vec<Rename>> {
    let pattern = if opts.find.is_empty() {
        None
    } else {
        let src = if opts.regex { opts.find.clone() } else { regex::escape(&opts.find) };
        let src = if opts.ignore_case { format!("(?i){src}") } else { src };
        Some(Regex::new(&src).context("invalid pattern")?)
    };
    let replacement = if opts.regex { opts.replace.clone() } else { opts.replace.replace('$', "$$") };

    let mut out = Vec::with_capacity(files.len());
    for (i, from) in files.iter().enumerate() {
        let Some(file_name) = from.file_name().and_then(|n| n.to_str()) else {
            out.push(Rename { from: from.clone(), to: from.clone(), problem: Some("Name isn't valid UTF-8".into()) });
            continue;
        };
        let (stem, ext) = match (opts.keep_extension, file_name.rsplit_once('.')) {
            (true, Some((s, e))) if !s.is_empty() => (s.to_string(), Some(e.to_string())),
            _ => (file_name.to_string(), None),
        };
        let mut name = match &pattern {
            Some(re) => re.replace_all(&stem, replacement.as_str()).into_owned(),
            None => stem,
        };
        if let Some(t) = &opts.template {
            name = expand(t, &name, ext.as_deref().unwrap_or(""), opts.start + i);
        }
        if let Some(e) = ext {
            name = format!("{name}.{e}");
        }
        out.push(Rename { to: from.with_file_name(&name), from: from.clone(), problem: validate(&name) });
    }

    flag_collisions(&mut out);
    Ok(out)
}

fn expand(template: &str, name: &str, ext: &str, n: usize) -> String {
    let re = Regex::new(r"\{n(?::(\d+))?\}").unwrap();
    let t = template.replace("{name}", name).replace("{ext}", ext);
    re.replace_all(&t, |c: &regex::Captures| {
        let width = c.get(1).and_then(|w| w.as_str().parse().ok()).unwrap_or(0);
        format!("{n:0width$}")
    })
    .into_owned()
}

fn validate(name: &str) -> Option<String> {
    if name.is_empty() || name == "." || name == ".." {
        Some("New name is empty".into())
    } else if name.contains('/') {
        Some("New name contains '/'".into())
    } else {
        None
    }
}

fn flag_collisions(renames: &mut [Rename]) {
    let sources: HashSet<PathBuf> = renames.iter().map(|r| r.from.clone()).collect();
    let mut seen = HashSet::new();
    for r in renames.iter_mut().filter(|r| r.problem.is_none() && r.changes()) {
        if !seen.insert(r.to.clone()) {
            r.problem = Some("Two files would get this name".into());
        } else if r.to.exists() && !sources.contains(&r.to) {
            r.problem = Some("A file with this name already exists".into());
        }
    }
}

/// Apply a plan. Refuses if any rename has a problem. Renames go through a
/// temporary name first so swaps (a→b, b→a) work.
pub fn apply(plan: &[Rename]) -> anyhow::Result<usize> {
    if let Some(r) = plan.iter().find(|r| r.problem.is_some()) {
        bail!("{}: {}", r.from.display(), r.problem.as_deref().unwrap());
    }
    let moves: Vec<&Rename> = plan.iter().filter(|r| r.changes()).collect();
    let mut staged = Vec::with_capacity(moves.len());
    for (i, r) in moves.iter().enumerate() {
        let tmp = temp_name(&r.from, i);
        std::fs::rename(&r.from, &tmp).with_context(|| format!("renaming {}", r.from.display()))?;
        staged.push((tmp, &r.to));
    }
    for (tmp, to) in &staged {
        std::fs::rename(tmp, to).with_context(|| format!("renaming to {}", to.display()))?;
    }
    Ok(staged.len())
}

fn temp_name(p: &Path, i: usize) -> PathBuf {
    p.with_file_name(format!(".forgekit-relabel-{}-{i}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(plan: &[Rename]) -> Vec<String> {
        plan.iter().map(|r| r.to.file_name().unwrap().to_str().unwrap().to_string()).collect()
    }

    #[test]
    fn plain_find_replace_keeps_extension() {
        let files = vec![PathBuf::from("/x/IMG_001.JPG"), PathBuf::from("/x/IMG_002.JPG")];
        let opts =
            Options { find: "IMG_".into(), replace: "Trip $1 ".into(), keep_extension: true, ..Default::default() };
        assert_eq!(names(&plan(&files, &opts).unwrap()), ["Trip $1 001.JPG", "Trip $1 002.JPG"]);
    }

    #[test]
    fn regex_and_template() {
        let files = vec![PathBuf::from("/x/a b.txt"), PathBuf::from("/x/c d.txt")];
        let opts = Options {
            find: r"(\w) (\w)".into(),
            replace: "$2-$1".into(),
            regex: true,
            keep_extension: true,
            template: Some("{n:2}_{name}".into()),
            start: 1,
            ..Default::default()
        };
        assert_eq!(names(&plan(&files, &opts).unwrap()), ["01_b-a.txt", "02_d-c.txt"]);
    }

    #[test]
    fn collisions_are_flagged() {
        let files = vec![PathBuf::from("/x/a1"), PathBuf::from("/x/a2")];
        let opts = Options { find: r"\d".into(), regex: true, ..Default::default() };
        let p = plan(&files, &opts).unwrap();
        assert!(p[0].problem.is_none());
        assert_eq!(p[1].problem.as_deref(), Some("Two files would get this name"));
    }

    #[test]
    fn apply_handles_swaps() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::write(&a, "A").unwrap();
        std::fs::write(&b, "B").unwrap();
        let p = vec![
            Rename { from: a.clone(), to: b.clone(), problem: None },
            Rename { from: b.clone(), to: a.clone(), problem: None },
        ];
        assert_eq!(apply(&p).unwrap(), 2);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "B");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "A");
    }
}
