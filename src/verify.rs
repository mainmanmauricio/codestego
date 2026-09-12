//! Post-embed safety verification.

use anyhow::{Result, bail};
use std::path::Path;
use std::process::Command;

use crate::lang::{self, LangProfile, non_trivia_bytes};

/// Assert non-trivia token bytes are unchanged.
pub fn assert_token_invariant(original: &str, modified: &str, profile: &LangProfile) -> Result<()> {
    let a = lang::lex(original, profile);
    let b = lang::lex(modified, profile);
    let ta = non_trivia_bytes(original, &a);
    let tb = non_trivia_bytes(modified, &b);
    if ta != tb {
        bail!("token stream changed after embed (would alter program semantics)");
    }
    Ok(())
}

/// Optionally shell out to a compiler. `cmd` may contain `{}` for the file path.
pub fn compile_check(cmd_template: &str, path: &Path) -> Result<()> {
    let path_str = path.to_string_lossy();
    let cmd = cmd_template.replace("{}", &path_str);
    let status = if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", &cmd]).status()
    } else {
        Command::new("sh").args(["-c", &cmd]).status()
    }
    .map_err(|e| anyhow::anyhow!("compile-check spawn failed: {e}"))?;
    if !status.success() {
        bail!("compile-check failed with status {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang;
    use tempfile::tempdir;

    #[test]
    fn token_invariant_ok_and_fail() {
        let profile = Lang::Rust.profile();
        let a = "fn main() { let x = 1; }\n";
        let b = "fn main() { let x = 1; } // c\n";
        assert!(assert_token_invariant(a, b, &profile).is_ok());
        let c = "fn main() { let x = 2; }\n";
        assert!(assert_token_invariant(a, c, &profile).is_err());
    }

    #[test]
    fn compile_check_true_false() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();
        assert!(compile_check("true", &path).is_ok());
        assert!(compile_check("false", &path).is_err());
    }
}
