mod descriptor;
mod reader;
mod sections;
pub mod tables;

pub use descriptor::*;
pub use reader::*;
pub use sections::*;
pub use tables::{TableBEntry, TableDEntry, Tables};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] binrw::Error),
    #[error("Fatal error: {0}")]
    Fatal(String),
    #[error("Not supported: {0}")]
    NotSupported(String),
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(binrw::Error::from(err))
    }
}
