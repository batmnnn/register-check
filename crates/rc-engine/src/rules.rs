use std::path::Path;

use sha2::{Digest, Sha256};

use crate::EngineError;

/// One static SQL rule. `id` comes from the first line, `-- R1 unbacked_mint` → `R1_unbacked_mint`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    /// Rule number N from `R<N>.sql`; rules run in this order.
    pub number: u32,
    pub sql: String,
    /// sha256 of the exact file bytes; this is what rc-attest registers on-chain.
    pub sql_hash: [u8; 32],
}

impl Rule {
    pub fn sql_hash_hex(&self) -> String {
        hex::encode(self.sql_hash)
    }
}

/// Parse one rule file. Only a single read-only SELECT is allowed: the SQL never takes parameters
/// and is not interpolated, so config can't inject SQL (red-team l).
pub fn parse_rule(file_name: &str, sql: &str) -> Result<Rule, EngineError> {
    let err = |reason: &str| EngineError::Rule {
        file: file_name.to_string(),
        reason: reason.to_string(),
    };
    let number = file_name
        .strip_prefix('R')
        .and_then(|s| s.strip_suffix(".sql"))
        .and_then(|n| n.parse::<u32>().ok())
        .ok_or_else(|| err("file name must be R<N>.sql"))?;
    let header = sql
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("-- "))
        .ok_or_else(|| err("first line must be `-- R<N> <name>`"))?;
    let mut parts = header.split_whitespace();
    let (tag, name) = match (parts.next(), parts.next(), parts.next()) {
        (Some(t), Some(n), None) => (t, n),
        _ => return Err(err("first line must be `-- R<N> <name>`")),
    };
    if tag != format!("R{number}") {
        return Err(err("header rule number does not match file name"));
    }
    if !name.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
        return Err(err("rule name must be lowercase snake_case"));
    }
    let body: String = sql
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    let body = body.trim();
    if !body.to_ascii_uppercase().starts_with("SELECT") {
        return Err(err("rule must be a single SELECT"));
    }
    if body.trim_end_matches(';').contains(';') {
        return Err(err("rule must be a single statement"));
    }
    Ok(Rule {
        id: format!("{tag}_{name}"),
        number,
        sql: body.trim_end_matches(';').to_string(),
        sql_hash: Sha256::digest(sql.as_bytes()).into(),
    })
}

/// Load every `R<N>.sql` in `dir`, sorted by N. Other files are ignored.
pub fn load_rules(dir: &Path) -> Result<Vec<Rule>, EngineError> {
    let io = |source| EngineError::Io {
        path: dir.to_path_buf(),
        source,
    };
    let mut rules = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(io)? {
        let path = entry.map_err(io)?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !(name.starts_with('R') && name.ends_with(".sql")) {
            continue;
        }
        let sql = std::fs::read_to_string(&path).map_err(|source| EngineError::Io {
            path: path.clone(),
            source,
        })?;
        rules.push(parse_rule(name, &sql)?);
    }
    rules.sort_by_key(|r| r.number);
    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_header_and_hashes_file_bytes() {
        let sql = "-- R7 some_rule\n-- comment\nSELECT 1\n";
        let r = parse_rule("R7.sql", sql).expect("parses");
        assert_eq!(r.id, "R7_some_rule");
        assert_eq!(r.number, 7);
        assert_eq!(r.sql, "SELECT 1");
        assert_eq!(r.sql_hash, <[u8; 32]>::from(Sha256::digest(sql.as_bytes())));
    }

    #[test]
    fn rejects_bad_rule_files() {
        for (file, sql) in [
            ("X1.sql", "-- R1 a\nSELECT 1"),
            ("R1.sql", "SELECT 1"),
            ("R1.sql", "-- R2 a\nSELECT 1"),
            ("R1.sql", "-- R1 Bad-Name\nSELECT 1"),
            ("R1.sql", "-- R1 a\nDROP TABLE mints"),
            ("R1.sql", "-- R1 a\nSELECT 1; DROP TABLE mints"),
            ("R1.sql", "-- R1 a\nCREATE VIEW v AS SELECT 1"),
        ] {
            assert!(
                matches!(parse_rule(file, sql), Err(EngineError::Rule { .. })),
                "{file}: {sql}"
            );
        }
    }

    #[test]
    fn loads_repo_rules_in_order() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rules");
        let ids: Vec<String> = load_rules(&dir)
            .expect("loads")
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(
            ids,
            [
                "R1_unbacked_mint",
                "R2_non_allowlisted_holder",
                "R3_holder_concentration",
                "R4_privileged_change"
            ]
        );
    }
}
