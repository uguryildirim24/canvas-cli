//! TLS front-end for wiremock; uses the production HTTPS checks unchanged.
use crate::{Client, GovernorConfig, Secret};
use std::ops::Deref;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        ServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    },
};

pub struct TestServer {
    mock: wiremock::MockServer,
    address: std::net::SocketAddr,
    task: tokio::task::JoinHandle<()>,
}
impl Deref for TestServer {
    type Target = wiremock::MockServer;
    fn deref(&self) -> &Self::Target {
        &self.mock
    }
}
impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl TestServer {
    pub async fn start() -> Self {
        Self::start_with_response(None).await
    }
    pub async fn raw(response: &'static [u8]) -> Self {
        Self::start_with_response(Some(response)).await
    }
    async fn start_with_response(response: Option<&'static [u8]>) -> Self {
        let mock = wiremock::MockServer::start().await;
        let upstream = *mock.address();
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from_pem_slice(include_bytes!("cert.pem")).unwrap()],
                PrivateKeyDer::from_pem_slice(include_bytes!("key.pem")).unwrap(),
            )
            .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    if let Ok(mut tls) = acceptor.accept(stream).await {
                        if let Some(response) = response {
                            use tokio::io::{AsyncReadExt, AsyncWriteExt};
                            let mut request = vec![0; 8192];
                            let _ = tls.read(&mut request).await;
                            tls.write_all(response).await.unwrap();
                            tls.shutdown().await.unwrap();
                        } else {
                            let mut tcp = TcpStream::connect(upstream).await.unwrap();
                            let _ = tokio::io::copy_bidirectional(&mut tls, &mut tcp).await;
                        }
                    }
                });
            }
        });
        Self {
            mock,
            address,
            task,
        }
    }
    pub fn uri(&self) -> String {
        format!("https://{}", self.address)
    }
}
pub fn test_client(server: &TestServer) -> Client {
    let mut client = Client::with_governor(
        server.uri().parse().unwrap(),
        Secret::new("tok"),
        "test",
        GovernorConfig {
            jitter: false,
            ..Default::default()
        },
    )
    .unwrap();
    let inner = Arc::get_mut(&mut client.inner).unwrap();
    let builder = || {
        reqwest::Client::builder()
            .no_proxy()
            .no_gzip()
            .no_brotli()
            .tls_certs_only([reqwest::Certificate::from_pem(include_bytes!("cert.pem")).unwrap()])
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(10))
    };
    inner.http = builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();
    inner.transfer_http = builder()
        .read_timeout(std::time::Duration::from_secs(60))
        .build()
        .unwrap();
    inner.upload_http = builder().build().unwrap();
    client
}
