//! High performance asynchronous TCP port forwarder over usbmuxd.
//!
//! Splices localhost:PORT (e.g. 8100, 9200) directly to the target device's
//! internal port using the usbmuxd protocol.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

#[cfg(unix)]
use tokio::net::UnixStream;

const USBMUXD_SOCKET_UNIX: &str = "/var/run/usbmuxd";
const USBMUXD_PORT_WIN: &str = "127.0.0.1:27015";

pub enum UsbmuxStream {
    #[cfg(unix)]
    Unix(UnixStream),
    Tcp(TcpStream),
}

impl AsyncRead for UsbmuxStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            UsbmuxStream::Unix(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            UsbmuxStream::Tcp(s) => std::pin::Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for UsbmuxStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            #[cfg(unix)]
            UsbmuxStream::Unix(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            UsbmuxStream::Tcp(s) => std::pin::Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            UsbmuxStream::Unix(s) => std::pin::Pin::new(s).poll_flush(cx),
            UsbmuxStream::Tcp(s) => std::pin::Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            UsbmuxStream::Unix(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            UsbmuxStream::Tcp(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}

pub async fn connect_usbmuxd() -> std::io::Result<UsbmuxStream> {
    #[cfg(unix)]
    {
        if std::path::Path::new(USBMUXD_SOCKET_UNIX).exists() {
            if let Ok(stream) = UnixStream::connect(USBMUXD_SOCKET_UNIX).await {
                return Ok(UsbmuxStream::Unix(stream));
            }
        }
        // Fallback to TCP if unix socket not accessible
        let stream = TcpStream::connect("127.0.0.1:27015").await?;
        let _ = stream.set_nodelay(true);
        Ok(UsbmuxStream::Tcp(stream))
    }
    #[cfg(windows)]
    {
        let stream = TcpStream::connect(USBMUXD_PORT_WIN).await?;
        Ok(UsbmuxStream::Tcp(stream))
    }
}

#[derive(Debug)]
pub struct ActiveTunnel {
    pub local_port: u16,
    pub device_port: u16,
    shutdown_tx: std::sync::Mutex<Option<oneshot::Sender<()>>>,
    is_alive: Arc<AtomicBool>,
}

impl ActiveTunnel {
    pub fn is_running(&self) -> bool {
        self.is_alive.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        if let Ok(mut lock) = self.shutdown_tx.lock() {
            if let Some(tx) = lock.take() {
                let _ = tx.send(());
            }
        }
        self.is_alive.store(false, Ordering::SeqCst);
    }
}

impl Drop for ActiveTunnel {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Spawns an asynchronous TCP tunnel from localhost:local_port to device:device_port.
pub async fn start_tunnel(
    local_port: u16,
    device_port: u16,
    device_id: u32,
    udid: String,
) -> std::io::Result<ActiveTunnel> {
    let addr: SocketAddr = format!("127.0.0.1:{}", local_port).parse().unwrap();
    let listener = TcpListener::bind(addr).await?;
    info!("✓ Tunnel established: localhost:{} -> {}:{}", local_port, udid, device_port);

    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
    let is_alive = Arc::new(AtomicBool::new(true));
    let alive_flag = is_alive.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    debug!("Tunnel localhost:{} shutdown received", local_port);
                    break;
                }
                accept_res = listener.accept() => {
                    match accept_res {
                        Ok((client_stream, client_addr)) => {
                            debug!("Accepted tunnel connection from {} on :{}", client_addr, local_port);
                            let _ = client_stream.set_nodelay(true);
                            tokio::spawn(async move {
                                if let Err(e) = forward_connection(client_stream, device_id, device_port).await {
                                    debug!("Tunnel forward error: {:?}", e);
                                }
                            });
                        }
                        Err(e) => {
                            warn!("Tunnel accept error on :{} : {:?}", local_port, e);
                            break;
                        }
                    }
                }
            }
        }
        alive_flag.store(false, Ordering::SeqCst);
    });

    Ok(ActiveTunnel {
        local_port,
        device_port,
        shutdown_tx: std::sync::Mutex::new(Some(shutdown_tx)),
        is_alive,
    })
}

async fn forward_connection(
    client_stream: TcpStream,
    device_id: u32,
    device_port: u16,
) -> anyhow::Result<()> {
    let mut mux = connect_usbmuxd().await?;

    // Send usbmuxd Connect packet
    let mut dict = plist::Dictionary::new();
    dict.insert("MessageType".into(), plist::Value::String("Connect".into()));
    dict.insert("ClientVersion".into(), plist::Value::Integer(7.into()));
    dict.insert("ProgName".into(), plist::Value::String("meridian-hub".into()));
    dict.insert("DeviceID".into(), plist::Value::Integer((device_id as u64).into()));
    dict.insert("PortNumber".into(), plist::Value::Integer(((device_port.to_be()) as u64).into()));

    let mut xml_buf = Vec::new();
    plist::to_writer_xml(&mut xml_buf, &dict)?;

    // Usbmuxd packet header: length (4), version (4 = 1), request (4 = 8), tag (4 = 1)
    let total_len = (16 + xml_buf.len()) as u32;
    let mut header = Vec::with_capacity(16);
    header.extend_from_slice(&total_len.to_le_bytes());
    header.extend_from_slice(&1u32.to_le_bytes()); // version 1
    header.extend_from_slice(&8u32.to_le_bytes()); // type 8 (plist)
    header.extend_from_slice(&1u32.to_le_bytes()); // tag 1

    mux.write_all(&header).await?;
    mux.write_all(&xml_buf).await?;
    mux.flush().await?;

    // Read 16-byte response header
    let mut resp_header = [0u8; 16];
    mux.read_exact(&mut resp_header).await?;
    let resp_len = u32::from_le_bytes([resp_header[0], resp_header[1], resp_header[2], resp_header[3]]) as usize;
    if resp_len > 16 {
        let payload_len = resp_len - 16;
        let mut resp_buf = vec![0u8; payload_len];
        mux.read_exact(&mut resp_buf).await?;

        if let Ok(plist::Value::Dictionary(dict)) = plist::from_bytes(&resp_buf) {
            if let Some(num) = dict.get("Number").and_then(|v| v.as_unsigned_integer()) {
                if num != 0 {
                    anyhow::bail!("usbmuxd connect rejected: code {}", num);
                }
            }
        }
    }

    // Bidirectional splice client <-> device
    let (mut client_r, mut client_w) = client_stream.into_split();
    let (mut mux_r, mut mux_w) = tokio::io::split(mux);

    let client_to_mux = tokio::io::copy(&mut client_r, &mut mux_w);
    let mux_to_client = tokio::io::copy(&mut mux_r, &mut client_w);

    tokio::select! {
        _ = client_to_mux => {},
        _ = mux_to_client => {},
    }

    Ok(())
}
