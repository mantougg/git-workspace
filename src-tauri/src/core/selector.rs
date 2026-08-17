//! Repository selector engine (T-20, Roadmap §52 Workspace Selection).
//!
//! Query syntax: whitespace-separated tokens, ANDed together:
//! - `@group:<name>`  — repositories in the named group
//! - `@tag:<tag>`     — repositories carrying the tag
//! - `@status:<s>`    — dirty / clean / conflict / ahead / behind / favorite
//! - any other token  — case-insensitive substring match on the repo name
//!
//! Filtering runs in memory over already-loaded repo facets (global
//! constraint §2: no DB full-table scan per keystroke).

/// The per-repository facets a selector matches against.
#[derive(Debug, Clone, Default)]
pub struct RepoFacet {
    pub path: String,
    pub name: String,
    pub group: Option<String>,
    pub tags: Vec<String>,
    pub dirty: bool,
    pub conflicted: bool,
    pub ahead: bool,
    pub behind: bool,
    pub favorite: bool,
}

/// One parsed selector token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectorToken {
    Group(String),
    Tag(String),
    Status(String),
    Text(String),
}

/// Parse a selector query into tokens. Unknown `@kind:` prefixes degrade to
/// plain text tokens (never an error — partial input while typing is normal).
pub fn parse_selector(query: &str) -> Vec<SelectorToken> {
    query
        .split_whitespace()
        .map(|tok| {
            if let Some(v) = tok.strip_prefix("@group:") {
                SelectorToken::Group(v.to_lowercase())
            } else if let Some(v) = tok.strip_prefix("@tag:") {
                SelectorToken::Tag(v.to_lowercase())
            } else if let Some(v) = tok.strip_prefix("@status:") {
                SelectorToken::Status(v.to_lowercase())
            } else {
                SelectorToken::Text(tok.to_lowercase())
            }
        })
        .collect()
}

/// Whether a repository matches all tokens (AND semantics).
pub fn matches_selector(facet: &RepoFacet, tokens: &[SelectorToken]) -> bool {
    tokens.iter().all(|tok| match tok {
        SelectorToken::Group(g) => facet
            .group
            .as_deref()
            .map(|fg| fg.to_lowercase() == *g)
            .unwrap_or(false),
        SelectorToken::Tag(t) => facet.tags.iter().any(|ft| ft.to_lowercase() == *t),
        SelectorToken::Status(s) => match s.as_str() {
            "dirty" => facet.dirty,
            "clean" => !facet.dirty,
            "conflict" => facet.conflicted,
            "ahead" => facet.ahead,
            "behind" => facet.behind,
            "favorite" => facet.favorite,
            _ => false,
        },
        SelectorToken::Text(t) => facet.name.to_lowercase().contains(t.as_str()),
    })
}

/// Filter facets by a selector query, returning matching repo paths.
pub fn select_paths(query: &str, facets: &[RepoFacet]) -> Vec<String> {
    let tokens = parse_selector(query);
    if tokens.is_empty() {
        return facets.iter().map(|f| f.path.clone()).collect();
    }
    facets
        .iter()
        .filter(|f| matches_selector(f, &tokens))
        .map(|f| f.path.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facet() -> RepoFacet {
        RepoFacet {
            path: "/ws/web-app".into(),
            name: "web-app".into(),
            group: Some("frontend".into()),
            tags: vec!["web".into(), "p0".into()],
            dirty: true,
            conflicted: false,
            ahead: true,
            behind: false,
            favorite: true,
        }
    }

    #[test]
    fn parses_token_kinds() {
        let tokens = parse_selector("@group:Frontend @tag:web @status:dirty app");
        assert_eq!(
            tokens,
            vec![
                SelectorToken::Group("frontend".into()),
                SelectorToken::Tag("web".into()),
                SelectorToken::Status("dirty".into()),
                SelectorToken::Text("app".into()),
            ]
        );
    }

    #[test]
    fn group_tag_status_text_combine_with_and() {
        let f = facet();
        assert!(matches_selector(&f, &parse_selector("@group:frontend")));
        assert!(!matches_selector(&f, &parse_selector("@group:backend")));
        assert!(matches_selector(&f, &parse_selector("@tag:web @tag:p0")));
        assert!(!matches_selector(&f, &parse_selector("@tag:web @tag:p1")));
        assert!(matches_selector(&f, &parse_selector("@status:dirty")));
        assert!(!matches_selector(&f, &parse_selector("@status:clean")));
        assert!(matches_selector(&f, &parse_selector("@status:ahead")));
        assert!(!matches_selector(&f, &parse_selector("@status:behind")));
        assert!(matches_selector(&f, &parse_selector("@status:favorite")));
        assert!(matches_selector(&f, &parse_selector("web")));
        assert!(!matches_selector(&f, &parse_selector("api")));
        // Combination across kinds: AND.
        assert!(matches_selector(
            &f,
            &parse_selector("@group:frontend @status:dirty web")
        ));
        assert!(!matches_selector(
            &f,
            &parse_selector("@group:frontend @status:clean web")
        ));
    }

    #[test]
    fn empty_query_matches_everything() {
        let facets = vec![facet(), RepoFacet {
            path: "/ws/api".into(),
            name: "api".into(),
            ..Default::default()
        }];
        assert_eq!(select_paths("", &facets).len(), 2);
        assert_eq!(
            select_paths("@status:favorite", &facets),
            vec!["/ws/web-app".to_string()]
        );
    }
}
