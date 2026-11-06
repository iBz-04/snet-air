use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};

use rustls::RootCertStore;
use rustls_pki_types::CertificateDer;
use rustls_pki_types::pem::PemObject;
use sha2::{Digest, Sha256};
use snet_pull::{CatalogModel, Fetcher, PullError, Pulled, pull, pulled_path};
use snet_testkit::certificates::{TestPki, Validity};
use snet_testkit::http::http_response;
use snet_testkit::tls::{Behaviour, TestServer, TlsVersion};

const BODY: &[u8] = b"GGUF pretend weights for a test";

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn loopback_only(host: &str) -> bool {
    host == "127.0.0.1"
}

/// A server whose n-th request gets the n-th scripted reply.
async fn server(pki: &TestPki, replies: Vec<Vec<u8>>) -> TestServer {
    let replies = Arc::new(replies);
    let behaviour = Behaviour::Handle(Arc::new(move |index, _request| replies.get(index).cloned()));
    let identity = pki
        .server_identity(&["127.0.0.1"], Validity::Current)
        .unwrap();
    TestServer::start(identity, TlsVersion::Tls13, behaviour)
        .await
        .unwrap()
}

fn fetcher(pki: &TestPki) -> Fetcher {
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from_pem_file(pki.ca_path()).unwrap())
        .unwrap();
    Fetcher::with_roots(roots, loopback_only).unwrap()
}

fn model(port: u16, bytes: u64, sha256: String) -> CatalogModel {
    CatalogModel {
        name: "test:1b",
        file: "test.gguf",
        url: Box::leak(
            format!("https://127.0.0.1:{port}/repo/resolve/main/test.gguf").into_boxed_str(),
        ),
        bytes,
        sha256: Box::leak(sha256.into_boxed_str()),
    }
}

#[tokio::test]
async fn a_redirected_download_is_verified_and_kept() {
    let pki = TestPki::new().unwrap();
    // The redirect names this same server, whose port is known only once
    // it listens.
    let port = Arc::new(AtomicU16::new(0));
    let handler_port = Arc::clone(&port);
    let server = TestServer::start(
        pki.server_identity(&["127.0.0.1"], Validity::Current)
            .unwrap(),
        TlsVersion::Tls13,
        Behaviour::Handle(Arc::new(move |index, _request| match index {
            0 => {
                let target = format!(
                    "https://127.0.0.1:{}/cdn/test.gguf?signed=1",
                    handler_port.load(Ordering::SeqCst)
                );
                Some(http_response(302, &[("location", &target)], b""))
            }
            1 => Some(http_response(200, &[], BODY)),
            _ => None,
        })),
    )
    .await
    .unwrap();
    port.store(server.port, Ordering::SeqCst);
    let home = tempfile::tempdir().unwrap();
    let model = model(server.port, BODY.len() as u64, sha256(BODY));
    let mut seen = 0;
    let pulled = pull(home.path(), &model, &fetcher(&pki), &mut |bytes| {
        seen = bytes
    })
    .await
    .unwrap();
    let path = home.path().join("models").join("test.gguf");
    assert_eq!(pulled, Pulled::Downloaded(path.clone()));
    assert_eq!(std::fs::read(&path).unwrap(), BODY);
    assert_eq!(seen, BODY.len() as u64);
    let requests = server.observations.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].path(), "/cdn/test.gguf?signed=1");

    let again = pull(home.path(), &model, &fetcher(&pki), &mut |_| {})
        .await
        .unwrap();
    assert_eq!(again, Pulled::AlreadyThere(path));
    assert_eq!(server.observations.requests().len(), 2);
}

#[tokio::test]
async fn a_file_with_the_wrong_hash_or_length_is_deleted() {
    for (bytes, sha256_hex) in [
        (BODY.len() as u64, sha256(b"other weights")),
        (BODY.len() as u64 + 1, sha256(BODY)),
    ] {
        let pki = TestPki::new().unwrap();
        let server = server(&pki, vec![http_response(200, &[], BODY)]).await;
        let home = tempfile::tempdir().unwrap();
        let model = model(server.port, bytes, sha256_hex);
        let error = pull(home.path(), &model, &fetcher(&pki), &mut |_| {})
            .await
            .unwrap_err();
        assert!(
            matches!(error, PullError::Checksum { .. } | PullError::Length { .. }),
            "{error}"
        );
        let leftovers: Vec<_> = std::fs::read_dir(home.path().join("models"))
            .unwrap()
            .collect();
        assert!(leftovers.is_empty(), "a failed download was kept");
        assert_eq!(pulled_path(home.path(), &model), None);
    }
}

#[tokio::test]
async fn a_redirect_off_the_model_host_is_refused() {
    let pki = TestPki::new().unwrap();
    let server = server(
        &pki,
        vec![http_response(
            302,
            &[("location", "https://evil.example/test.gguf")],
            b"",
        )],
    )
    .await;
    let home = tempfile::tempdir().unwrap();
    let model = model(server.port, BODY.len() as u64, sha256(BODY));
    let error = pull(home.path(), &model, &fetcher(&pki), &mut |_| {})
        .await
        .unwrap_err();
    assert!(
        matches!(error, PullError::HostRefused { ref host } if host == "evil.example"),
        "{error}"
    );
}

#[tokio::test]
async fn an_error_status_fails_the_pull() {
    let pki = TestPki::new().unwrap();
    let server = server(&pki, vec![http_response(404, &[], b"not found")]).await;
    let home = tempfile::tempdir().unwrap();
    let model = model(server.port, BODY.len() as u64, sha256(BODY));
    let error = pull(home.path(), &model, &fetcher(&pki), &mut |_| {})
        .await
        .unwrap_err();
    assert!(
        matches!(error, PullError::Status { status: 404 }),
        "{error}"
    );
}
