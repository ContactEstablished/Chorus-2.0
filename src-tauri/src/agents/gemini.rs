//! Read-only folder trust for Gemini CLI 0.64.0 (Q28). Unknown input leaves Gemini's prompt live.

use std::path::Path;

use chorus_core::gemini::{self, TrustLevel};

const MAX_TRUST_BYTES: u64 = 4 * 1024 * 1024;

pub fn launch_trusted(parent: &[(String, String)], cwd: &str, repository: Option<&str>) -> Result<bool, String> {
    // The child receives USERPROFILE, not an ambient GEMINI_CLI_HOME (the environment allow-list).
    let Some(home) = super::env_value(parent, "USERPROFILE") else { return Ok(false) };
    let path = Path::new(&home).join(".gemini").join("trustedFolders.json");
    match std::fs::metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err("Gemini's trusted-folders file couldn't be read; its own trust check applies.".into()),
        Ok(m) if m.len() > MAX_TRUST_BYTES => return Err("Gemini's trusted-folders file is too large to check; its own trust check applies.".into()),
        Ok(_) => {}
    }
    let config = std::fs::read_to_string(&path).map_err(|_| "Gemini's trusted-folders file couldn't be read; its own trust check applies.")?;
    let mut rules = gemini::trust_rules(&config)
        .ok_or("Gemini's trusted-folders format isn't recognised; its own trust check applies.")?;
    for rule in &mut rules {
        let raw = Path::new(&rule.path);
        if !raw.is_absolute() {
            return Err("Gemini has a relative trust rule; its own trust check applies.".into());
        }
        let effective = if rule.level == TrustLevel::TrustParent { raw.parent().unwrap_or(raw) } else { raw };
        // Resolve junctions and short paths, like Gemini's realpathSync. If resolution fails,
        // do not infer trust from a lexical prefix that may refer to a different folder.
        rule.path = real_path(effective)?;
    }
    let cwd = real_path(Path::new(cwd))?;
    let repository = repository.map(|p| real_path(Path::new(p))).transpose()?;
    Ok(gemini::launch_trusted(&rules, &cwd, repository.as_deref()))
}

fn real_path(path: &Path) -> Result<String, String> {
    std::fs::canonicalize(path).map(|p| p.display().to_string())
        .map_err(|_| "A Gemini trust path couldn't be resolved; its own trust check applies.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_is_read_only_and_unknown_folders_are_not_trusted() {
        let dir = std::env::temp_dir().join(format!("chorus-gemini-trust-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".gemini")).unwrap();
        let repo = dir.join("repo");
        let wt = dir.join("worktree");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&wt).unwrap();
        let env = vec![("USERPROFILE".into(), dir.display().to_string())];
        let config = dir.join(".gemini/trustedFolders.json");
        assert!(!launch_trusted(&env, &wt.display().to_string(), None).unwrap());
        assert!(!config.exists());
        let text = serde_json::to_string(&serde_json::json!({repo.display().to_string(): "TRUST_FOLDER"})).unwrap();
        std::fs::write(&config, &text).unwrap();
        assert!(launch_trusted(&env, &wt.display().to_string(), Some(&repo.display().to_string())).unwrap());
        assert!(!launch_trusted(&env, &wt.display().to_string(), None).unwrap());
        assert_eq!(std::fs::read_to_string(&config).unwrap(), text);
        std::fs::write(&config, "not json").unwrap();
        assert!(launch_trusted(&env, &wt.display().to_string(), None).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
