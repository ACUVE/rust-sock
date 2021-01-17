use super::utils::{determine_connection_type, ConnectionType};
use std::convert::AsRef;
use std::ffi::OsStr;
use std::io::{self};
use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpSocket, UnixStream};

async fn get_connection_impl<S: AsRef<OsStr>>(
    server: S,
) -> io::Result<(Box<dyn AsyncRead + Unpin>, Box<dyn AsyncWrite + Unpin>)> {
    use ConnectionType::*;

    match determine_connection_type(server) {
        Some(Ip(addr)) => {
            let socket = match addr {
                SocketAddr::V4(_) => TcpSocket::new_v4()?,
                SocketAddr::V6(_) => TcpSocket::new_v6()?,
            };
            let stream = socket.connect(addr).await?;
            let (write, read) = stream.into_split();
            Ok((Box::new(write), Box::new(read)))
        }
        Some(Unix(path)) => {
            let stream = UnixStream::connect(path).await?;
            let (write, read) = stream.into_split();
            Ok((Box::new(write), Box::new(read)))
        }
        None => Err(io::Error::new(io::ErrorKind::NotConnected, "Unkown server")),
    }
}

pub async fn get_connection<T, U>(
    servers: T,
) -> io::Result<(Box<dyn AsyncRead + Unpin>, Box<dyn AsyncWrite + Unpin>)>
where
    T: IntoIterator<Item = U>,
    U: AsRef<OsStr>,
{
    for server in servers {
        let ret = get_connection_impl(server).await;
        if ret.is_ok() {
            return ret;
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotConnected,
        "Cannot connect all servers",
    ))
}
