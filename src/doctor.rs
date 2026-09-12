//! Warn about formatters/hooks that destroy chosen carriers.

use std::path::Path;

use anyhow::Result;

use crate::carriers::CarrierKind;

#[derive(Debug)]
pub struct DoctorWarning {
    pub path: String,
    pub message: String,
    pub affects: Vec<&'static str>,
}

pub fn inspect(roots: &[impl AsRef<Path>], carriers: &[CarrierKind]) -> Result<Vec<DoctorWarning>> {
    let mut warnings = Vec::new();
    for root in roots {
        let root = root.as_ref();
        check_editorconfig(root, carriers, &mut warnings);
        check_file(root, ".prettierrc", carriers, &mut warnings, &["eol", "comment-space"]);
        check_file(root, ".prettierrc.js", carriers, &mut warnings, &["eol", "comment-space"]);
        check_file(root, ".prettierrc.json", carriers, &mut warnings, &["eol", "comment-space"]);
        check_file(root, ".clang-format", carriers, &mut warnings, &["eol", "comment-space"]);
        check_file(root, ".rustfmt.toml", carriers, &mut warnings, &["eol"]);
        check_file(root, "rustfmt.toml", carriers, &mut warnings, &["eol"]);
        check_gofmt(root, carriers, &mut warnings);
        check_markup(root, carriers, &mut warnings);
        check_pre_commit(root, carriers, &mut warnings);
    }
    Ok(warnings)
}

fn wants(carriers: &[CarrierKind], name: &str) -> bool {
    carriers.iter().any(|c| c.name() == name)
}

fn check_editorconfig(root: &Path, carriers: &[CarrierKind], out: &mut Vec<DoctorWarning>) {
    let path = root.join(".editorconfig");
    if !path.is_file() {
        // also search upward one level
        return;
    }
    if let Ok(text) = std::fs::read_to_string(&path) {
        if text.to_ascii_lowercase().contains("trim_trailing_whitespace")
            && text.lines().any(|l| {
                let t = l.trim().to_ascii_lowercase();
                t.starts_with("trim_trailing_whitespace") && t.contains("true")
            })
            && wants(carriers, "eol")
        {
            out.push(DoctorWarning {
                path: path.display().to_string(),
                message: "editorconfig trim_trailing_whitespace=true will destroy eol carrier"
                    .into(),
                affects: vec!["eol"],
            });
        }
    }
}

fn check_file(
    root: &Path,
    name: &str,
    carriers: &[CarrierKind],
    out: &mut Vec<DoctorWarning>,
    affects: &[&'static str],
) {
    let path = root.join(name);
    if !path.is_file() {
        return;
    }
    let relevant: Vec<&'static str> = affects
        .iter()
        .copied()
        .filter(|n| wants(carriers, n))
        .collect();
    if relevant.is_empty() {
        return;
    }
    out.push(DoctorWarning {
        path: path.display().to_string(),
        message: format!("{name} present; formatters often strip trailing whitespace / reflow comments"),
        affects: relevant,
    });
}

fn check_gofmt(root: &Path, carriers: &[CarrierKind], out: &mut Vec<DoctorWarning>) {
    if !wants(carriers, "eol") {
        return;
    }
    // If there are .go files, warn that gofmt strips trailing whitespace
    let has_go = std::fs::read_dir(root)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .any(|e| e.path().extension().map(|x| x == "go").unwrap_or(false))
        })
        .unwrap_or(false);
    if has_go {
        out.push(DoctorWarning {
            path: root.display().to_string(),
            message: "gofmt strips trailing whitespace — eol carrier will not survive gofmt".into(),
            affects: vec!["eol"],
        });
    }
}

fn check_markup(root: &Path, carriers: &[CarrierKind], out: &mut Vec<DoctorWarning>) {
    let relevant: Vec<&'static str> = ["comment-zw", "comment-space", "eol"]
        .into_iter()
        .filter(|n| wants(carriers, n))
        .collect();
    if relevant.is_empty() {
        return;
    }
    let has_markup = std::fs::read_dir(root)
        .map(|rd| {
            rd.filter_map(|e| e.ok()).any(|e| {
                crate::lang::Lang::from_ext(&e.path())
                    .is_some_and(|l| matches!(l, crate::lang::Lang::Html | crate::lang::Lang::Xml | crate::lang::Lang::Sgml))
            })
        })
        .unwrap_or(false);
    if has_markup {
        out.push(DoctorWarning {
            path: root.display().to_string(),
            message: "HTML/XML minifiers, tidy, and xmllint --format strip comments or rewrite whitespace"
                .into(),
            affects: relevant,
        });
    }
}

fn check_pre_commit(root: &Path, carriers: &[CarrierKind], out: &mut Vec<DoctorWarning>) {
    let path = root.join(".pre-commit-config.yaml");
    if !path.is_file() {
        return;
    }
    if let Ok(text) = std::fs::read_to_string(&path) {
        if text.contains("trailing-whitespace") && wants(carriers, "eol") {
            out.push(DoctorWarning {
                path: path.display().to_string(),
                message: "pre-commit trailing-whitespace hook will destroy eol carrier".into(),
                affects: vec!["eol"],
            });
        }
        if text.contains("prettier") {
            let relevant: Vec<&'static str> = ["eol", "comment-space"]
                .into_iter()
                .filter(|n| wants(carriers, n))
                .collect();
            if !relevant.is_empty() {
                out.push(DoctorWarning {
                    path: path.display().to_string(),
                    message: "pre-commit prettier hook may destroy whitespace carriers".into(),
                    affects: relevant,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn editorconfig_warns_eol() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join(".editorconfig"),
            "[*]\ntrim_trailing_whitespace = true\n",
        )
        .unwrap();
        let warnings = inspect(&[dir.path()], &[CarrierKind::Eol]).unwrap();
        assert!(
            warnings.iter().any(|w| w.affects.contains(&"eol")),
            "{warnings:?}"
        );
    }

    #[test]
    fn prettierrc_warns_space() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join(".prettierrc"), "{}\n").unwrap();
        let warnings = inspect(&[dir.path()], &[CarrierKind::CommentSpace]).unwrap();
        assert!(
            warnings.iter().any(|w| w.affects.contains(&"comment-space")),
            "{warnings:?}"
        );
    }

    #[test]
    fn markup_files_warn_minifiers() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<p><!-- c --></p>\n").unwrap();
        let warnings = inspect(&[dir.path()], &[CarrierKind::CommentZw]).unwrap();
        assert!(
            warnings.iter().any(|w| w.message.contains("minifiers")),
            "{warnings:?}"
        );
    }
}
