//! Verifies structured authentication and provider response classification.

use super::*;

#[test]
fn github_auth_error_json_never_means_ready() {
    let bytes = br#"{"hosts":{"github.com":[{"state":"error","active":true,"host":"github.com","error":"private diagnostic"}]}}"#;
    assert_eq!(classify_auth(bytes), Err(PrCode::AuthUnavailable));
}

#[test]
fn github_auth_requires_one_supported_active_target_account() {
    for (json, expected) in [
        (r#"{"hosts":{}}"#, Err(PrCode::AuthRequired)),
        (r#"{"hosts":{"github.com":[]}}"#, Err(PrCode::AuthRequired)),
        (r#"{"hosts":{"github.com":[{"host":"github.com","active":false,"state":"success"}]}}"#, Err(PrCode::AuthRequired)),
        (r#"{"hosts":{"github.com":[{"host":"github.com","active":true,"state":"timeout"}]}}"#, Err(PrCode::AuthUnavailable)),
        (r#"{"hosts":{"github.com":[{"host":"github.com","active":true,"state":"success"}]}}"#, Ok(())),
        (r#"{"hosts":{"github.com":[{"host":"elsewhere","active":true,"state":"success"}]}}"#, Err(PrCode::GhUnsupported)),
        (r#"{"hosts":{"github.com":[{"host":"github.com","active":true,"state":"future"}]}}"#, Err(PrCode::GhUnsupported)),
        (r#"{"hosts":{"github.com":[{"host":"github.com","active":true,"state":"success"},{"host":"github.com","active":true,"state":"success"}]}}"#, Err(PrCode::GhUnsupported)),
        (r#"{"accounts":[]}"#, Err(PrCode::GhUnsupported)),
        ("not JSON", Err(PrCode::InvalidOutput)),
    ] { assert_eq!(classify_auth(json.as_bytes()), expected); }
}

#[test]
fn github_version_requires_structured_auth_capability_release() {
    for version in ["gh version 2.81.0 (2025-10-01)\nhttps://github.com/cli/cli/releases/tag/v2.81.0", "gh version 2.100.2"] { assert!(classify_version(version.as_bytes(),true).is_ok()); }
    for version in ["gh version 2.80.9", "gh version 1.99.0", "gh version 2.81.0-unknown", "2.81.0", "gh version 2.81", "gh version 2.x.0"] { assert_eq!(classify_version(version.as_bytes(),true),Err(PrCode::GhUnsupported)); }
    assert_eq!(classify_version(b"gh version 2.81.0",false),Err(PrCode::GhUnsupported));
}

fn api(status: u16, headers: &str, body: &str, success: bool) -> Result<ApiResponse, Failure> {
    parse_api(format!("HTTP/2.0 {status} Status\r\n{headers}\r\n{body}").as_bytes(),success,100)
}
fn code(result: Result<ApiResponse, Failure>) -> PrCode { match result { Err(failure) => failure.code, Ok(_) => panic!("expected classified failure") } }

#[test]
fn github_status_and_rate_headers_classify_without_stderr() {
    assert_eq!(parse_api(b"HTTP/2.0 200 OK\nContent-Type: application/json\r\n\r\n{}",true,100).unwrap_or_else(|_|panic!()).status,200);
    assert_eq!(parse_api(b"HTTP/2.0 200 OK\n\r\n{}",true,100).unwrap_or_else(|_|panic!()).status,200);
    assert_eq!(code(api(403,"x-ratelimit-remaining: 1\r\n","{}",false)),PrCode::AccessDenied);
    let failure = match api(403,"x-ratelimit-remaining: 0\r\nx-ratelimit-reset: 120\r\nRetry-After: 30\r\n","{}",false) { Err(f) => f, Ok(_) => panic!() };
    assert_eq!(failure.code,PrCode::RateLimited);
    assert_eq!(failure.retry_at,Some(130_000));
    let failure = match api(429,"Retry-After: Thu, 01 Jan 1970 00:03:00 GMT\r\n","{}",false) { Err(f) => f, Ok(_) => panic!() };
    assert_eq!(failure.retry_at,Some(180_000));
    for (status,expected) in [(401,PrCode::AuthRequired),(404,PrCode::RepositoryUnavailable),(500,PrCode::Network),(501,PrCode::GhUnsupported),(302,PrCode::InvalidOutput)] { assert_eq!(code(api(status,"","{}",false)),expected); }
    let response = api(200,"content-type: application/json; charset=utf-8\r\nx-ratelimit-remaining: 42\r\n","{\"id\":1}",true).unwrap_or_else(|_| panic!());
    assert_eq!(response.status,200); assert_eq!(response.rate.remaining,Some(42)); assert_eq!(response.body["id"],1);
}

#[test]
fn github_malformed_framing_and_ambiguous_headers_never_publish() {
    for bytes in [b"HTTP/2.0 200 OK\n\nHTTP/2.0 200 OK\n\n{}".as_slice(), b"HTTP/2.0 200 OK\nfolded\n\n{}", b"HTTP/2.0 200 OK\n\n{\"x\":\"\xff\"}", b"HTTP/2.0 099 OK\n\n{}", b"HTTP/2.0 200 OK\n\n{}{}"] {
        assert_eq!(code(parse_api(bytes,true,100)),PrCode::InvalidOutput);
    }
    for headers in ["retry-after: 1\r\nRetry-After: 2\r\n", "x-ratelimit-remaining: -1\r\n", "Retry-After: tomorrow\r\n", "Retry-After: 18446744073709551615\r\n", "x-ratelimit-reset: x\r\n", "Content-Type: text/html\r\n", " bad-header: value\r\n"] {
        assert_eq!(code(api(200,headers,"{}",true)),PrCode::InvalidOutput);
    }
    assert_eq!(code(api(200,"","{}",false)),PrCode::InvalidOutput);
    assert_eq!(code(parse_api(b"",false,100)),PrCode::Network);
}

#[test]
fn github_graphql_partial_errors_are_not_success_or_empty_results() {
    for (kind,expected) in [("FORBIDDEN",PrCode::AccessDenied),("RATE_LIMITED",PrCode::RateLimited),("NOT_FOUND",PrCode::RepositoryUnavailable),("UNAUTHENTICATED",PrCode::AuthRequired),("unknown",PrCode::InvalidOutput)] {
        assert_eq!(code(api(200,"",&format!(r#"{{"data":{{"viewer":null}},"errors":[{{"type":"{kind}","message":"private"}}]}}"#),true)),expected);
    }
}

#[test]
fn github_graphql_authentication_failures_dominate_rate_errors_in_either_order() {
    for kinds in [["UNAUTHENTICATED","RATE_LIMITED"],["RATE_LIMITED","UNAUTHENTICATED"],["UNAUTHENTICATED","FORBIDDEN"],["FORBIDDEN","UNAUTHENTICATED"]] {
        let body = serde_json::json!({"errors":kinds.map(|kind| serde_json::json!({"type":kind}))}).to_string();
        assert_eq!(code(api(200,"",&body,false)),PrCode::AuthRequired);
    }
    for kinds in [["FORBIDDEN","RATE_LIMITED"],["RATE_LIMITED","FORBIDDEN"]] {
        let body = serde_json::json!({"errors":kinds.map(|kind| serde_json::json!({"type":kind}))}).to_string();
        assert_eq!(code(api(200,"",&body,false)),PrCode::AccessDenied);
    }
}

#[test]
fn github_json_collection_and_nesting_are_bounded_before_normalization() {
    let large = serde_json::json!({"items":vec![0;MAX_JSON_COLLECTION+1]}).to_string();
    assert_eq!(code(api(200,"",&large,true)),PrCode::ResourceLimit);
    let deep = format!("{}0{}", "[".repeat(MAX_JSON_DEPTH+1), "]".repeat(MAX_JSON_DEPTH+1));
    assert_eq!(code(api(200,"",&deep,true)),PrCode::ResourceLimit);
}

#[test]
fn github_routes_are_fixed_gets_and_query_data_is_encoded() {
    let reads = [
        GhRead::ReadRepository{owner:"owner".into(),repository:"repo".into()},
        GhRead::ListPulls{owner:"owner".into(),repository:"repo".into(),head:Some("fork:branch/&state=closed?@x{owner}".into()),page:2},
        GhRead::ReadPull{owner:"owner".into(),repository:"repo".into(),number:2},
        GhRead::ReadPullFiles{owner:"owner".into(),repository:"repo".into(),number:2,page:1},
        GhRead::ReadCommit{owner:"owner".into(),repository:"repo".into(),oid:"a".repeat(40),page:1},
    ];
    for read in reads {
        let input = request::build(&read).unwrap();
        assert!(input.args.windows(2).any(|p| p == ["--method","GET"]));
        assert!(input.args.windows(2).any(|p| p == ["--hostname","github.com"]));
        assert!(!input.args.iter().any(|arg| arg == "--paginate"));
        assert!(input.input.is_none());
        if matches!(read,GhRead::ListPulls{..}) { assert!(input.args[1].contains("head=fork%3Abranch%2F%26state%3Dclosed%3F%40x%7Bowner%7D")); }
    }
    for owner in ["../bad", "https://host", "{owner}", "", "@file"] { assert!(request::build(&GhRead::ReadRepository{owner:owner.into(),repository:"repo".into()}).is_err()); }
    assert!(request::build(&GhRead::ReadPull{owner:"owner".into(),repository:"repo".into(),number:0}).is_err());
    assert!(request::build(&GhRead::ReadCommit{owner:"owner".into(),repository:"repo".into(),oid:"main".into(),page:1}).is_err());
}

#[test]
fn github_graphql_variables_cannot_change_bundled_documents() {
    for connection in [Connection::Commits,Connection::Timeline,Connection::Threads,Connection::ThreadComments{thread_id:"opaque-thread".into()},Connection::Reviewers,Connection::Labels] {
        let cursor = "@private\" } mutation { deleteRepository }";
        let input = request::build(&GhRead::ReadConnection { owner:"owner".into(),repository:"repo".into(),number:1,connection,cursor:Some(cursor.into()) }).unwrap();
        assert!(input.args.windows(2).any(|p| p == ["--input","-"]));
        let body: serde_json::Value = serde_json::from_slice(input.input.as_ref().unwrap()).unwrap();
        assert_eq!(body["variables"]["cursor"],cursor);
        assert_eq!(body["variables"]["count"],100);
        assert!(!body["query"].as_str().unwrap().contains(cursor));
        assert!(body["query"].as_str().unwrap().starts_with("query "));
    }
}

#[test]
fn github_malformed_pagination_cannot_certify_exhausted_lookup() {
    for link in ["not a link", "<https://elsewhere.invalid/repos/o/r/pulls?page=2>; rel=\"next\"",
        "<https://user:secret@api.github.com/repos/o/r/pulls?page=2>; rel=\"next\"",
        "<https://api.github.com/repos/o/r/pulls?page=2>; rel=\"next\", <https://api.github.com/repos/o/r/pulls?page=3>; rel=\"next\""] {
        assert_eq!(code(api(200,&format!("Link: {link}\r\n"),"[]",true)),PrCode::InvalidOutput);
    }
}

#[test]
fn github_pagination_preserves_next_page_even_for_an_empty_body() {
    let response = api(200,"Link: <https://api.github.com/repos/o/r/pulls?head=o%3Atopic&page=2>; rel=\"next\", <https://api.github.com/repos/o/r/pulls?page=3>; rel=\"last\"\r\n","[]",true).unwrap_or_else(|_|panic!());
    assert!(response.has_next);
    assert!(!api(200,"Link: <https://api.github.com/repos/o/r/pulls?page=1>; rel=\"prev\"\r\n","[]",true).unwrap_or_else(|_|panic!()).has_next);
    assert!(!api(200,"","[]",true).unwrap_or_else(|_|panic!()).has_next);
    assert!(api(200,"Link: <https://api.github.com/repos/o/r/pulls?page=2>; rel=\"next\"\r\nLink: <https://api.github.com/repos/o/r/pulls?page=3>; rel=\"last\"\r\n","[]",true).is_err());
}
