use std::net::SocketAddr;

use aionui_common::generate_id;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tracing::debug;

pub struct GuideMcpServer {
    http_addr: SocketAddr,
    auth_token: String,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl GuideMcpServer {
    pub async fn start() -> Result<Self, String> {
        let auth_token = generate_id();
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("Failed to bind guide MCP HTTP listener: {e}"))?;
        let http_addr = listener
            .local_addr()
            .map_err(|e| format!("Failed to read guide MCP local addr: {e}"))?;

        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        tokio::spawn(accept_loop(listener, shutdown_rx));

        debug!(http_port = http_addr.port(), "Guide MCP Server started");

        Ok(Self {
            http_addr,
            auth_token,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    pub fn http_port(&self) -> u16 {
        self.http_addr.port()
    }

    pub fn http_addr(&self) -> SocketAddr {
        self.http_addr
    }

    pub fn auth_token(&self) -> &str {
        &self.auth_token
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
            debug!(http_port = self.http_addr.port(), "Guide MCP Server stop requested");
        }
    }
}

impl Drop for GuideMcpServer {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn accept_loop(listener: TcpListener, mut shutdown_rx: oneshot::Receiver<()>) {
    loop {
        tokio::select! {
            _ = &mut shutdown_rx => {
                debug!("Guide MCP Server shutting down");
                break;
            }
            accept = listener.accept() => {
                // D26b/c will plug in JSON-RPC / HTTP dispatch for
                // aion_create_team + aion_list_models. For the skeleton we
                // accept and immediately drop the connection.
                if let Ok((stream, _peer)) = accept {
                    drop(stream);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpStream;
    use tokio::time::timeout;

    #[tokio::test]
    async fn start_returns_positive_port_and_token() {
        let server = GuideMcpServer::start().await.expect("start should succeed");
        assert!(server.http_port() > 0, "http_port should be assigned");
        assert!(!server.auth_token().is_empty(), "auth_token should be generated");
    }

    #[tokio::test]
    async fn each_start_uses_a_fresh_auth_token() {
        let a = GuideMcpServer::start().await.unwrap();
        let b = GuideMcpServer::start().await.unwrap();
        assert_ne!(a.auth_token(), b.auth_token());
    }

    #[tokio::test]
    async fn stop_closes_the_listener() {
        let mut server = GuideMcpServer::start().await.unwrap();
        let port = server.http_port();
        server.stop();

        // Give the accept loop a moment to observe the shutdown signal.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Either the connect is refused outright, or it succeeds but the
        // listener-less port yields an immediate EOF on read. Both are
        // acceptable evidence that the server is no longer serving.
        match timeout(Duration::from_millis(200), TcpStream::connect(("127.0.0.1", port))).await {
            Ok(Ok(mut stream)) => {
                let mut buf = [0u8; 1];
                let read = timeout(Duration::from_millis(200), stream.read(&mut buf)).await;
                match read {
                    Ok(Ok(0)) => { /* EOF — expected */ }
                    Ok(Err(_)) => { /* connection error — expected */ }
                    Ok(Ok(_)) => panic!("unexpected data from stopped server"),
                    Err(_) => panic!("server still reading after stop"),
                }
            }
            Ok(Err(_)) => { /* connection refused — expected */ }
            Err(_) => panic!("connect timed out (expected refuse or EOF)"),
        }
    }

    #[tokio::test]
    async fn stop_is_idempotent() {
        let mut server = GuideMcpServer::start().await.unwrap();
        server.stop();
        server.stop();
    }
}
