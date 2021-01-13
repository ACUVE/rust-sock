extern crate rust_sock;

use clap::clap_app;
use rust_sock::default;
use rust_sock::message::{Request, Response};
use rust_sock::utils::{
    determine_connection_type, read_serialized, write_serialized, ConnectionType,
};
use std::convert::AsRef;
use std::error::Error;
use std::ffi::OsStr;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use tokio::fs;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::net::{TcpSocket, UnixStream};
use tokio::runtime;
use tokio::time::timeout;

async fn get_connection_impl<S: AsRef<OsStr>>(
    server: S,
) -> Result<(Box<dyn AsyncRead + Unpin>, Box<dyn AsyncWrite + Unpin>), Box<dyn Error>> {
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
        None => Err("Unkown server".into()),
    }
}

async fn get_connection<T, U>(
    servers: T,
) -> Result<(Box<dyn AsyncRead + Unpin>, Box<dyn AsyncWrite + Unpin>), Box<dyn Error>>
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
    Err("Cannot connect all servers".into())
}

fn main() -> Result<(), Box<dyn Error>> {
    let app = clap_app!(client =>
        (@arg server: -s --server +takes_value "Server")
        (@arg timeout: -t --timeout +takes_value "Timeout seconds")
        (@subcommand SendFile =>
            (about: "Send file to Server")
            (@arg FILE: +required "Sent file")
        )
        (@subcommand Echo =>
            (about: "Output on Server")
            (@arg STRING: +required "String to be show")
        )
    );
    let matches = app.get_matches();

    let servers = match matches.values_of_os("server") {
        Some(servers) => servers.map(OsStr::to_os_string).collect(),
        None => default::server().unwrap(),
    };
    let timeout_sec = match matches.value_of("timeout") {
        Some(ref str) => str.parse().expect("cannot parse timeout"),
        None => 10,
    };

    let rt = runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let (mut read, mut write) = get_connection(servers.iter()).await?;

        let req = match matches.subcommand() {
            ("SendFile", Some(sub)) => {
                let filepath = Path::new(sub.value_of_os("FILE").unwrap());
                let mut file = fs::OpenOptions::new().read(true).open(&filepath).await?;
                let filesize = file.metadata().await?.len();
                let mut buffer = vec![0; filesize as usize];
                file.read_exact(&mut buffer).await?;
                Request::SendFile {
                    filename: filepath.file_name().unwrap().to_string_lossy().into_owned(),
                    data: buffer.into(),
                }
            }
            ("Echo", Some(sub)) => {
                let string = sub.value_of_lossy("STRING").unwrap().into();
                Request::Echo { string }
            }
            (_unknown, Some(_sub)) => {
                unimplemented!()
            }
            _ => {
                return Err("Please set subcommand".into());
            }
        };

        timeout(
            Duration::from_secs(timeout_sec),
            write_serialized(&mut write, &req),
        )
        .await??;
        let response = timeout(
            Duration::from_secs(timeout_sec),
            read_serialized::<Response, _>(&mut read),
        )
        .await??;

        println!("{:?}", response);

        Ok(())
    })
}
