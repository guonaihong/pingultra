use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PingError {
    #[error("Failed to send packet: {0}")]
    SendError(#[from] io::Error),
    
    #[error("Invalid address: {0}")]
    InvalidAddress(String),
    
    #[error("Timeout")]
    Timeout,
    
    #[error("Packet construction error")]
    PacketConstructionError,
    
    #[error("Permission denied: raw sockets require root privileges")]
    PermissionDenied,
    
    #[error("Failed to resolve hostname: {0}")]
    ResolutionError(String),
    
    #[error("Other error: {0}")]
    Other(String),
}

#[allow(dead_code)]
pub type PingResult<T> = Result<T, PingError>;
