use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum Request {
    Ping,
    OpenVSCode { path: String },
    SendFile { filename: String, data: Box<[u8]> },
    Echo { string: String },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Ok,
    Err(String),
}
