use std::error::Error;
use std::ffi::OsStr;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug)]
pub enum ReadSerializedError {
    Bincode(bincode::Error),
    Io(std::io::Error),
    Closed,
}
impl std::fmt::Display for ReadSerializedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use ReadSerializedError::*;
        match self {
            Bincode(ref error) => write!(f, "Bincode({})", error),
            Io(ref error) => write!(f, "Io({})", error),
            Closed => write!(f, "Closed"),
        }
    }
}
impl Error for ReadSerializedError {}

pub async fn read_serialized<O, R>(read: &mut R) -> Result<O, ReadSerializedError>
where
    O: serde::de::DeserializeOwned,
    R: AsyncReadExt + Unpin,
{
    use ReadSerializedError::*;

    match read.read_u128().await {
        Ok(size) => {
            let mut buff = vec![0u8; size as _];
            read.read_exact(&mut buff).await.map_err(Io)?;
            bincode::deserialize::<O>(&buff).map_err(Bincode)
        }
        Err(ref err) if err.kind() == std::io::ErrorKind::UnexpectedEof => Err(Closed),
        Err(err) => Err(Io(err)),
    }
}

#[derive(Debug)]
pub enum WriteSerializedError {
    Bincode(bincode::Error),
    Io(std::io::Error),
}
impl std::fmt::Display for WriteSerializedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use WriteSerializedError::*;
        match self {
            Bincode(ref error) => write!(f, "Bincode({})", error),
            Io(ref error) => write!(f, "Io({})", error),
        }
    }
}
impl Error for WriteSerializedError {}

pub async fn write_serialized<O, W>(
    write: &mut W,
    serializable: O,
) -> Result<(), WriteSerializedError>
where
    O: serde::ser::Serialize,
    W: AsyncWriteExt + Unpin,
{
    use WriteSerializedError::*;
    let buff = bincode::serialize(&serializable).map_err(Bincode)?;
    write.write_u128(buff.len() as _).await.map_err(Io)?;
    write.write_all(&buff).await.map_err(Io)?;
    write.flush().await.map_err(Io)?;
    Ok(())
}

#[derive(Debug)]
pub enum ConnectionType {
    Ip(SocketAddr),
    Unix(PathBuf),
}
pub fn determine_connection_type<S: AsRef<OsStr>>(server_str: S) -> Option<ConnectionType> {
    use ConnectionType::*;

    let server_str = server_str.as_ref();
    if let Some(ipcand) = server_str.to_str() {
        if let Ok(ip) = ipcand.parse() {
            return Some(Ip(ip));
        }
    }
    let path = Path::new(server_str);
    if path.is_absolute() {
        return Some(Unix(path.into()));
    }
    None
}
