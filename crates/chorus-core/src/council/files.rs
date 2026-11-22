//! Which of a project's files a Council participant may read (D137). Pure: the service hands in
//! the candidate list (git's tracked and unignored files, or a walk) and every requested path.

/// A requested path as a clean project-relative path (`src/main.rs`), or the reason it is refused.
/// An empty result is the project root. A leading `/` or `./` is read as the root, because models
/// often write paths that way; a drive, a `:` (alternate streams) or `..` is refused.
pub fn normalize(input: &str) -> Result<String, String> {
    if input.contains('\0') {
        return Err("That path contains a null byte.".into());
    }
    if input.contains(':') {
        return Err("Paths are relative to the project's root folder; drives and absolute paths can't be read.".into());
    }
    let mut parts = Vec::new();
    for part in input.split(['/', '\\']) {
        match part.trim() {
            "" | "." => {}
            ".." => return Err("Paths can't leave the project's folder (`..`).".into()),
            p => parts.push(p),
        }
    }
    Ok(parts.join("/"))
}

/// Why a project-relative file is never readable, whatever git says about it.
pub fn blocked(rel: &str) -> Option<&'static str> {
    let lower = rel.to_ascii_lowercase();
    let parts: Vec<&str> = lower.split('/').collect();
    if parts.contains(&".git") {
        return Some("Git's own folder is not readable.");
    }
    let name = parts.last().copied().unwrap_or("");
    let env_ok = [".env.example", ".env.sample", ".env.template", ".env.dist"];
    if (name == ".env" || name.starts_with(".env.")) && !env_ok.contains(&name) {
        return Some("Environment files can hold secrets, so they are never readable.");
    }
    const NAMES: [&str; 9] =
        [".npmrc", ".netrc", "_netrc", ".pypirc", ".git-credentials", ".dockercfg", ".htpasswd", ".pgpass", "secrets.json"];
    if NAMES.contains(&name) || name.starts_with("credentials") || name.starts_with("secrets.") || name == ".secrets" {
        return Some("Credential files are never readable.");
    }
    if ["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"].iter().any(|k| name.starts_with(k)) {
        return Some("SSH keys are never readable.");
    }
    const EXTENSIONS: [&str; 13] =
        ["pem", "key", "pfx", "p12", "p8", "der", "jks", "keystore", "ppk", "kdbx", "tfstate", "tfvars", "asc"];
    if name.rsplit_once('.').is_some_and(|(_, ext)| EXTENSIONS.contains(&ext)) {
        return Some("Key, certificate and secret-store files are never readable.");
    }
    None
}

/// Folders a walk outside git skips: build output and dependencies, not source.
pub fn skip_dir(name: &str) -> bool {
    const SKIP: [&str; 20] = [
        ".git", "node_modules", "target", "dist", "build", "bin", "obj", "out", ".next", ".nuxt", "coverage", ".venv", "venv",
        "__pycache__", ".idea", ".vs", ".vscode", ".gradle", ".cache", ".turbo",
    ];
    SKIP.contains(&name.to_ascii_lowercase().as_str())
}

/// A file is treated as binary when its first 8 KB hold a NUL byte (git's own test).
pub fn looks_binary(head: &[u8]) -> bool {
    head.iter().take(8192).any(|b| *b == 0)
}

/// Files larger than this are refused by reason rather than read.
pub const MAX_FILE_BYTES: u64 = 2_000_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_reads_models_paths_and_refuses_escapes() {
        assert_eq!(normalize("/src//main.rs").unwrap(), "src/main.rs");
        assert_eq!(normalize(".\\src\\lib.rs").unwrap(), "src/lib.rs");
        assert_eq!(normalize("").unwrap(), "");
        assert_eq!(normalize("./").unwrap(), "");
        assert!(normalize("../secret").is_err());
        assert!(normalize("src/../../x").is_err());
        assert!(normalize("C:\\Windows\\win.ini").is_err());
        assert!(normalize("file.txt:stream").is_err());
        assert!(normalize("a\0b").is_err());
    }

    #[test]
    fn secrets_are_blocked_whatever_their_folder() {
        for p in [".env", "app/.env.local", "x/.git/config", "certs/server.PEM", "deploy/id_ed25519.pub", ".npmrc", "infra/prod.tfvars", "config/credentials.json", "secrets.yaml"] {
            assert!(blocked(p).is_some(), "{p}");
        }
        for p in [".env.example", "src/environment.ts", "docs/keys.md", "src/secretsManager.ts", "README.md", "src/keyboard.rs"] {
            assert!(blocked(p).is_none(), "{p}");
        }
    }

    #[test]
    fn walk_skips_and_binary() {
        assert!(skip_dir("node_modules") && skip_dir("Target") && !skip_dir("src"));
        assert!(looks_binary(b"PK\x03\x04\0\0") && !looks_binary(b"fn main() {}"));
    }
}
