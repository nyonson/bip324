//! Helper functions for bitcoin p2p proxies.
//!
//! The V1 and V2 p2p protocols have different header encodings, so a proxy has to do
//! a little more work than just encrypt/decrypt.

use std::fmt;
use std::net::SocketAddr;

use bitcoin::consensus::{Decodable, Encodable};
pub use bitcoin::p2p::message::RawNetworkMessage;
use bitcoin::p2p::{Address, Magic};
use hex::prelude::*;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

/// Default to local host on port 1324.
pub const DEFAULT_PROXY: &str = "127.0.0.1:1324";
/// Default to the signet network.
const DEFAULT_MAGIC: Magic = Magic::BITCOIN;
/// All V1 messages have a 24 byte header.
const V1_HEADER_BYTES: usize = 24;
/// Hex encoding of ascii version command.
const VERSION_COMMAND: [u8; 12] = [
    0x76, 0x65, 0x72, 0x73, 0x69, 0x6f, 0x6e, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// An error occured while establishing the proxy connection or during the main loop.
#[derive(Debug)]
pub enum Error {
    WrongNetwork,
    WrongCommand,
    Network(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::WrongNetwork => write!(f, "Recieved message on wrong network"),
            Error::Network(e) => write!(f, "Network error {}", e),
            Error::WrongCommand => write!(f, "Recieved message with wrong command"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Network(e) => Some(e),
            Error::WrongNetwork => None,
            Error::WrongCommand => None,
        }
    }
}

// Convert IO errors.
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Network(e)
    }
}

/// Peek the input stream and pluck the remote address based on the version message.
pub async fn peek_addr(client: &TcpStream) -> Result<SocketAddr, Error> {
    println!("Validating client connection.");
    // Peek the first 70 bytes, 24 for the header and 46 for the first part of the version message.
    let mut peek_bytes = [0; 70];
    client.peek(&mut peek_bytes).await?;

    // Check network magic.
    println!("Got magic: {}", &peek_bytes[0..4].to_lower_hex_string());
    if DEFAULT_MAGIC.to_bytes().ne(&peek_bytes[0..4]) {
        return Err(Error::WrongNetwork);
    }

    // Check command.
    println!("Got command: {}", &peek_bytes[4..16].to_lower_hex_string());
    if VERSION_COMMAND.ne(&peek_bytes[4..16]) {
        return Err(Error::WrongCommand);
    }

    // Pull off address.
    let mut addr_bytes = &peek_bytes[44..];
    let remote_addr = Address::consensus_decode(&mut addr_bytes).expect("network address bytes");
    let socket_addr = remote_addr.socket_addr().expect("IP");

    Ok(socket_addr)
}

/// Read a network message off of the input stream.
pub async fn read_v1<T: AsyncRead + Unpin>(input: &mut T) -> Result<RawNetworkMessage, Error> {
    let mut header_bytes = [0; V1_HEADER_BYTES];
    input.read_exact(&mut header_bytes).await?;

    let payload_len = u32::from_le_bytes(
        header_bytes[16..20]
            .try_into()
            .expect("4 header length bytes"),
    );

    let mut payload = vec![0u8; payload_len as usize];
    input.read_exact(&mut payload).await?;
    let mut full_message = header_bytes.to_vec();
    full_message.append(&mut payload);
    let message = RawNetworkMessage::consensus_decode(&mut full_message.as_slice())
        .expect("raw network message");

    Ok(message)
}

/// Write the network message to the output stream.
pub async fn write_v1<T: AsyncWrite + Unpin>(
    output: &mut T,
    msg: RawNetworkMessage,
) -> Result<(), Error> {
    let mut write_bytes = vec![];
    msg.consensus_encode(&mut write_bytes)
        .expect("write to vector");
    Ok(output.write_all(&write_bytes).await?)
}
