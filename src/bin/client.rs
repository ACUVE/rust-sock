extern crate rust_sock;

use clap::clap_app;
use rust_sock::connection::get_connection;
use rust_sock::default;
use rust_sock::message::{Request, Response};
use rust_sock::utils::{read_serialized, write_serialized};
use std::error::Error;
use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::runtime;
use tokio::time::timeout;

fn main() -> Result<(), Box<dyn Error>> {
    let app = clap_app!(client =>
        (@arg server: -s --server +takes_value "Server")
        (@arg timeout: -t --timeout +takes_value "Timeout seconds")
        (@subcommand Ping =>
            (about: "Do nothing")
        )
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
            ("Ping", Some(_)) => Request::Ping,
            ("SendFile", Some(sub)) => {
                let filepath = Path::new(sub.value_of_os("FILE").unwrap());
                let mut file = fs::OpenOptions::new().read(true).open(&filepath).await?;
                let filesize = file.metadata().await?.len();
                let mut buffer = vec![0; filesize as _];
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
