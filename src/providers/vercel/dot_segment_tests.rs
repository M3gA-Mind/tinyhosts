//! A dot-only identifier must not reach a Vercel URL path: `..` (or `%2E%2E`,
//! which the URL parser decodes the same way) would address the parent route.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use wiremock::MockServer;

use super::*;

fn host(server: &MockServer) -> Vercel {
    Vercel::with_base_url(Credentials::new("token").unwrap(), server.uri()).unwrap()
}

#[tokio::test]
async fn dot_only_identifiers_are_refused_before_any_request() {
    let server = MockServer::start().await;
    let host = host(&server);
    let vars = [EnvVar::new("KEY", "value")];

    for id in ["", ".", ".."] {
        assert_eq!(
            host.find_site(id).await.unwrap_err(),
            Error::InvalidIdentifier
        );
        assert_eq!(
            host.set_env(id, &vars).await.unwrap_err(),
            Error::InvalidIdentifier
        );
        assert_eq!(
            host.deployment(id).await.unwrap_err(),
            Error::InvalidIdentifier
        );
        assert_eq!(
            host.list_domains(id).await.unwrap_err(),
            Error::InvalidIdentifier
        );
    }

    assert!(server.received_requests().await.unwrap().is_empty());
}
