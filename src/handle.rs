use super::message::{Request, Response};
use std::error::Error;
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncWriteExt;

async fn handle(request: Request) -> Result<(), Box<dyn Error>> {
    use Request::*;
    match request {
        OpenVSCode { path: ref _path } => {
            unimplemented!()
        }
        SendFile {
            ref filename,
            ref data,
        } => {
            println!("SendFile: filename={} data.len()={}", filename, data.len());
            let path = Path::new(filename);
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(path.file_name().ok_or("No filename")?)
                .await?;
            file.write_all(data).await?;
            Ok(())
        }
    }
}

pub async fn handle_request(request: Request) -> Response {
    use Response::*;
    handle(request)
        .await
        .map_or_else(|err| Err(err.to_string()), |_| Ok)
}
