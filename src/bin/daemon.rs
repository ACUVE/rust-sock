extern crate rust_sock;

use clap::clap_app;
use rust_sock::default;
use rust_sock::handle::handle_request;
use rust_sock::utils::{
    determine_connection_type, read_serialized, write_serialized, ConnectionType,
    ReadSerializedError,
};
use std::error::Error;
use std::ffi::OsStr;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UnixListener};
use tokio::runtime;
use tokio::signal;
use tokio::time;

// とりあえず1分ぐらい
const TIMEOUT: u64 = 60;

struct RequestHandler<R, W>
where
    R: AsyncReadExt + Unpin + Send + Sync,
    W: AsyncWriteExt + Unpin + Send + Sync,
{
    read: R,
    write: W,
}

impl<R, W> RequestHandler<R, W>
where
    R: AsyncReadExt + Unpin + Send + Sync,
    W: AsyncWriteExt + Unpin + Send + Sync,
{
    fn new(read: R, write: W) -> RequestHandler<R, W> {
        Self {
            read: read,
            write: write,
        }
    }

    async fn handle(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        loop {
            match time::timeout(
                Duration::from_secs(TIMEOUT),
                read_serialized(&mut self.read),
            )
            .await
            {
                Ok(Ok(request)) => {
                    let response = handle_request(request).await;
                    time::timeout(
                        Duration::from_secs(TIMEOUT),
                        write_serialized(&mut self.write, &response),
                    )
                    .await??
                }
                Ok(Err(ReadSerializedError::Closed)) => break,
                Ok(Err(err)) => return Err(err.into()),
                Err(err) => return Err(err.into()),
            };
        }
        Ok(())
    }

    async fn handle_stream(read: R, write: W) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut handler = Self::new(read, write);
        let ret = handler.handle().await;
        println!("{:?}", ret);
        ret
    }
}

async fn spawn_listeners<T, U>(
    servers: T,
) -> Result<
    Arc<Mutex<Vec<tokio::task::JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>>>>,
    Box<dyn Error + Send + Sync>,
>
where
    T: IntoIterator<Item = U>,
    U: AsRef<OsStr>,
{
    use tokio::spawn;
    let handlers = Arc::new(Mutex::new(Vec::new()));
    for server in servers.into_iter() {
        let server = server.as_ref();
        let handlers_tmp = handlers.clone();
        match determine_connection_type(server) {
            Some(ConnectionType::Ip(addr)) => {
                let listener = TcpListener::bind(addr).await?;
                handlers.lock().unwrap().push(spawn(async move {
                    loop {
                        let (stream, addr) = listener.accept().await?;
                        println!("TcpListener: connected from {:?}", addr);
                        let (read, write) = stream.into_split();
                        let handle = spawn(RequestHandler::handle_stream(read, write));
                        handlers_tmp.lock().unwrap().push(handle);
                    }
                }));
            }
            Some(ConnectionType::Unix(path)) => {
                let listener = UnixListener::bind(path)?;
                handlers.lock().unwrap().push(spawn(async move {
                    loop {
                        let (stream, addr) = listener.accept().await?;
                        println!("UnixListener: connected from {:?}", addr);
                        let (read, write) = stream.into_split();
                        let handle = spawn(RequestHandler::handle_stream(read, write));
                        handlers_tmp.lock().unwrap().push(handle);
                    }
                }));
            }
            None => Err("Cannot determine server type".to_owned())?,
        }
    }
    Ok(handlers)
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let app = clap_app!(client =>
        (@arg server: -s --server +takes_value "Server")
        (@arg timeout: -t --timeout +takes_value "Timeout seconds")
    );
    let matches = app.get_matches();

    let rt = runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let (_tmpdir, servers) = match matches.values_of_os("server") {
            Some(iter) => (None, iter.into_iter().map(|v| v.into()).collect()),
            None => {
                let (tmpdir, path) = default::new_unix_path()?;
                (Some(tmpdir), vec![path])
            }
        };
        spawn_listeners(&servers).await?;

        {
            let stdout = io::stdout();
            let mut stdout = stdout.lock();
            stdout.write(b"RUST_SOCK=")?;
            stdout.write(
                &servers
                    .iter()
                    .map(|s| s.as_bytes())
                    .collect::<Vec<_>>()
                    .join(&b":"[0]),
            )?;
            stdout.write(b"\n")?;
            stdout.flush()?;
        }

        signal::ctrl_c().await?;
        println!("ctrl-c received!");

        for server in servers.iter() {
            if let Some(ConnectionType::Unix(path)) = determine_connection_type(server) {
                tokio::fs::remove_file(path).await?;
            }
        }

        Ok(())
    })
}
