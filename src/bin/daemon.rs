extern crate rust_sock;

use clap::clap_app;
use rust_sock::default;
use rust_sock::handle::handle_request;
use rust_sock::utils::{
    determine_connection_type, ping_test_to_servers, read_serialized, write_serialized,
    ConnectionType, ReadSerializedError,
};
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs::{self};
use std::io::{self, BufRead, BufReader, ErrorKind, Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
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

fn write_socks<'a, T, U, W>(mut write: W, list: T) -> io::Result<()>
where
    T: IntoIterator<Item = &'a U>,
    U: AsRef<OsStr> + 'a,
    W: Write,
{
    let mut iter = list.into_iter();
    if let Some(obj) = iter.next() {
        write.write(obj.as_ref().as_bytes())?;
        for string in iter {
            write.write(b",")?;
            write.write(string.as_ref().as_bytes())?;
        }
    }
    Ok(())
}

async fn check_already_run() -> io::Result<Option<Box<[OsString]>>> {
    let app_dir = default::application_dir();
    if !app_dir.exists() {
        fs::create_dir(&app_dir)?;
    }
    let sock_path_txt = app_dir.join(Path::new("sock.txt"));
    match fs::OpenOptions::new().read(true).open(&sock_path_txt) {
        Ok(file) => {
            let mut vec = Vec::new();
            let file = BufReader::new(file);
            for server in file.split(b',') {
                let server = <OsString as OsStringExt>::from_vec(server?);
                if !ping_test_to_servers(&[&server]).await? {
                    return Ok(None);
                }
                vec.push(server.to_owned());
            }
            Ok(Some(vec.into()))
        }
        Err(ref err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn get_socket_file_path() -> io::Result<PathBuf> {
    let app_dir = default::application_dir();
    if !app_dir.exists() {
        fs::create_dir(&app_dir)?;
    }
    let sock_path_txt = app_dir.join(Path::new("sock.txt"));
    Ok(sock_path_txt)
}

async fn create_sock_file<'a, T, U>(list: T) -> io::Result<()>
where
    T: IntoIterator<Item = &'a U>,
    U: AsRef<OsStr> + 'a,
{
    let sock_path_txt = get_socket_file_path()?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(sock_path_txt)?;
    write_socks(&mut file, list)?;
    file.flush()?;
    Ok(())
}

fn stdout_servers<'a, T, U>(servers: T) -> io::Result<()>
where
    T: IntoIterator<Item = &'a U>,
    U: AsRef<OsStr> + 'a,
{
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    stdout.write(b"RUST_SOCK=")?;
    write_socks(&mut stdout, servers)?;
    stdout.write(b"\n")?;
    stdout.flush()?;
    Ok(())
}

async fn remove_sock_file<'a, T, U>(servers: T) -> io::Result<()>
where
    T: IntoIterator<Item = &'a U>,
    U: AsRef<OsStr> + 'a,
{
    let sock_path_txt = get_socket_file_path()?;
    let real = match fs::OpenOptions::new().read(true).open(&sock_path_txt) {
        Ok(file) => {
            let mut file = BufReader::new(file);
            let mut real = Vec::new();
            file.read_to_end(&mut real)?;
            real
        }
        Err(ref err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    let mut reference = Vec::new();
    write_socks(&mut reference, servers)?;
    if reference == real {
        fs::remove_file(&sock_path_txt)?;
    }
    Ok(())
}

async fn remove_unix_socket_file<'a, T, U>(servers: T) -> io::Result<()>
where
    T: IntoIterator<Item = &'a U>,
    U: AsRef<OsStr> + 'a,
{
    for server in servers {
        if let Some(ConnectionType::Unix(path)) = determine_connection_type(server) {
            tokio::fs::remove_file(path).await?;
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let rt = runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        if let Some(ref list) = check_already_run().await? {
            stdout_servers(list.into_iter())?;
            return Ok(());
        }

        let app = clap_app!(client =>
            (@arg server: -s --server +takes_value "Server")
            (@arg timeout: -t --timeout +takes_value "Timeout seconds")
        );
        let matches = app.get_matches();

        let (_tmpdir, servers) = match matches.values_of_os("server") {
            Some(iter) => (None, iter.into_iter().map(|v| v.into()).collect()),
            None => {
                let (tmpdir, path) = default::new_unix_path()?;
                (Some(tmpdir), vec![path])
            }
        };
        spawn_listeners(&servers).await?;

        create_sock_file(&servers).await?;
        stdout_servers(&servers)?;

        signal::ctrl_c().await?;
        println!("ctrl-c received!");

        remove_sock_file(&servers).await?;
        remove_unix_socket_file(&servers).await?;

        Ok(())
    })
}
