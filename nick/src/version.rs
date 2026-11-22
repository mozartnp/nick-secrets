//! The version of the nick (NS-52): the one of Cargo.toml and the commit it was built from,
//! which build.rs gives as NICK_GIT_COMMIT (`desconhecido` without git, without a repository or
//! without a commit). Shown by `nick --version` and on the screen "Configurações".

/// `<version> (commit <hash>)`, e.g. `0.1.0 (commit abc1234)`.
pub fn long() -> String {
    format!(
        "{} (commit {})",
        env!("CARGO_PKG_VERSION"),
        env!("NICK_GIT_COMMIT")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether `text` is `<digits>.<digits>.<digits>`.
    fn is_semver(text: &str) -> bool {
        let parts: Vec<&str> = text.split('.').collect();
        parts.len() == 3
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    }

    /// Whether `text` is a short hash (7 or more lowercase hex digits) or `desconhecido`.
    fn is_commit(text: &str) -> bool {
        text == "desconhecido"
            || (text.len() >= 7
                && text
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    }

    #[test]
    fn the_long_version_has_the_version_and_the_commit() {
        let long = long();
        let (version, rest) = long.split_once(' ').unwrap_or((&long, ""));
        assert!(is_semver(version), "{long}");
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
        let commit = rest
            .strip_prefix("(commit ")
            .and_then(|rest| rest.strip_suffix(')'))
            .unwrap_or_else(|| panic!("{long}"));
        assert!(is_commit(commit), "{long}");
    }

    #[test]
    fn the_commit_check_knows_a_hash_from_anything_else() {
        assert!(is_commit("abc1234"));
        assert!(is_commit("desconhecido"));
        assert!(!is_commit("abc123"));
        assert!(!is_commit("ABC1234"));
        assert!(!is_commit("abc123g"));
        assert!(is_semver("0.1.0"));
        assert!(!is_semver("0.1"));
        assert!(!is_semver("0.1.x"));
    }
}
