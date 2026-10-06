//! Covers supported URL authority and rejects ambiguous routing input.
use super::*;
#[test]
fn github_association_urls_accept_only_literal_github_routing() {
    for url in [
        "https://github.com/fork/project.git",
        "git@github.com:fork/project.git",
        "ssh://git@github.com/fork/project",
    ] {
        assert_eq!(
            local::parse_url(url),
            Some(RepositoryName {
                owner: "fork".into(),
                name: "project".into()
            })
        );
    }
    for url in [
        "https://token@github.com/fork/project",
        "https://github.com/fork/project?token=x",
        "https://github.com/fork/project#fragment",
        "git@work-alias:fork/project.git",
        "https://github.com/fork/project/extra",
        "https://github.com/fork/%70roject",
        "file:///tmp/project",
        "ssh://git@github.com:22/fork/project",
    ] {
        assert!(local::parse_url(url).is_none());
    }
}
#[test]
fn github_association_explicit_refs_reject_revision_expressions() {
    for value in ["feature", "feature/nested", "renamed-branch"] {
        assert!(local::validate_ref(value).is_ok());
    }
    for value in [
        "",
        "main~1",
        "refs/heads/main:other",
        "@{push}",
        "a..b",
        "a.lock",
        "a//b",
        "-unsafe",
    ] {
        assert!(local::validate_ref(value).is_err());
    }
}
