// Copyright 2020 - developers of the `grammers` project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tracing::info;

use super::ServerAddr;

pub enum NetStream {
    Tcp(TcpStream),
    MtProxy {
        stream: tokio::io::DuplexStream,
        task: tokio::task::JoinHandle<()>,
    },
    #[cfg(feature = "proxy")]
    ProxySocks5(tokio_socks::tcp::Socks5Stream<TcpStream>),
}

impl NetStream {
    pub(crate) fn split(
        &mut self,
    ) -> (
        tokio::io::ReadHalf<&mut Self>,
        tokio::io::WriteHalf<&mut Self>,
    ) {
        tokio::io::split(self)
    }

    pub(crate) async fn connect(addr: &ServerAddr) -> Result<Self, std::io::Error> {
        info!("connecting...");
        match addr {
            ServerAddr::MtProxy { config, dc_id } => {
                let (stream, task) = crate::mtproxy::connect(config, *dc_id).await?;
                Ok(Self::MtProxy { stream, task })
            }
            ServerAddr::Tcp { address } => Ok(NetStream::Tcp(TcpStream::connect(address).await?)),
            #[cfg(feature = "proxy")]
            ServerAddr::Proxied { address, proxy } => {
                Self::connect_proxy_stream(address, proxy).await
            }
        }
    }

    #[cfg(feature = "proxy")]
    async fn connect_proxy_stream(
        addr: &std::net::SocketAddr,
        proxy_url: &str,
    ) -> Result<NetStream, std::io::Error> {
        use std::{
            io::{self, ErrorKind},
            net::{IpAddr, SocketAddr},
        };

        use hickory_resolver::Resolver;
        use url::Host;

        let proxy = url::Url::parse(proxy_url)
            .map_err(|err| io::Error::new(ErrorKind::InvalidData, err))?;
        let scheme = proxy.scheme();
        let host = proxy.host().ok_or(io::Error::new(
            ErrorKind::NotFound,
            format!("proxy host is missing from url: {}", proxy_url),
        ))?;
        let port = proxy.port().ok_or(io::Error::new(
            ErrorKind::NotFound,
            format!("proxy port is missing from url: {}", proxy_url),
        ))?;
        let username = proxy.username();
        let password = proxy.password().unwrap_or("");
        let socks_addr = match host {
            Host::Domain(domain) => {
                let resolver = Resolver::builder_tokio().unwrap().build().unwrap();
                let response = resolver.lookup_ip(domain).await.map_err(io::Error::other)?;
                let socks_ip_addr = response.iter().next().ok_or(io::Error::new(
                    ErrorKind::NotFound,
                    format!("proxy host did not return any ip address: {}", domain),
                ))?;
                SocketAddr::new(socks_ip_addr, port)
            }
            Host::Ipv4(v4) => SocketAddr::new(IpAddr::from(v4), port),
            Host::Ipv6(v6) => SocketAddr::new(IpAddr::from(v6), port),
        };

        match scheme {
            "socks5" => {
                if username.is_empty() {
                    Ok(NetStream::ProxySocks5(
                        tokio_socks::tcp::Socks5Stream::connect(socks_addr, addr)
                            .await
                            .map_err(|err| io::Error::new(ErrorKind::ConnectionAborted, err))?,
                    ))
                } else {
                    Ok(NetStream::ProxySocks5(
                        tokio_socks::tcp::Socks5Stream::connect_with_password(
                            socks_addr, addr, username, password,
                        )
                        .await
                        .map_err(|err| io::Error::new(ErrorKind::ConnectionAborted, err))?,
                    ))
                }
            }
            scheme => Err(io::Error::new(
                ErrorKind::ConnectionAborted,
                format!("proxy scheme not supported: {}", scheme),
            )),
        }
    }

    pub(crate) async fn disconnect(&mut self) -> std::io::Result<()> {
        match self {
            Self::MtProxy { stream, .. } => stream.shutdown().await,
            Self::Tcp(stream) => stream.shutdown().await,
            #[cfg(feature = "proxy")]
            Self::ProxySocks5(stream) => stream.shutdown().await,
        }
    }
}

impl Drop for NetStream {
    fn drop(&mut self) {
        if let Self::MtProxy { task, .. } = self {
            task.abort();
        }
    }
}
impl tokio::io::AsyncRead for NetStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Tcp(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            Self::MtProxy { stream, .. } => std::pin::Pin::new(stream).poll_read(cx, buf),
            #[cfg(feature = "proxy")]
            Self::ProxySocks5(s) => std::pin::Pin::new(s).poll_read(cx, buf),
        }
    }
}
impl tokio::io::AsyncWrite for NetStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Self::Tcp(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            Self::MtProxy { stream, .. } => std::pin::Pin::new(stream).poll_write(cx, buf),
            #[cfg(feature = "proxy")]
            Self::ProxySocks5(s) => std::pin::Pin::new(s).poll_write(cx, buf),
        }
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Tcp(s) => std::pin::Pin::new(s).poll_flush(cx),
            Self::MtProxy { stream, .. } => std::pin::Pin::new(stream).poll_flush(cx),
            #[cfg(feature = "proxy")]
            Self::ProxySocks5(s) => std::pin::Pin::new(s).poll_flush(cx),
        }
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Tcp(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            Self::MtProxy { stream, .. } => std::pin::Pin::new(stream).poll_shutdown(cx),
            #[cfg(feature = "proxy")]
            Self::ProxySocks5(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}
