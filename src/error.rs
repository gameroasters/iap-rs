//! Convenience types for lib specific error handling
#![allow(clippy::pub_enum_variant_names)]

use thiserror::Error;

/// General Error type that will wrap other error types for our convenience.
#[derive(Error, Debug)]
pub enum Error {
    /// serde_json Errors
    #[error("serde_json error: {0}")]
    SerdeError(#[from] serde_json::Error),

    /// hyper::http errors
    #[error("http error: {0}")]
    HttpError(#[from] hyper::http::Error),

    /// hyper errors
    #[error("hyper error: {0}")]
    HyperError(#[from] hyper::Error),

    /// yup_oauth2 errors
    #[error("yup_oauth error: {0}")]
    YupOauth2Error(#[from] yup_oauth2::Error),

    /// std io errors
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),

    /// Parse int errors
    #[error("parse int error: {0}")]
    ParseIntError(#[from] std::num::ParseIntError),

    /// From utf8 errors
    #[error("utf8 error: {0}")]
    FromUtf8Error(#[from] std::string::FromUtf8Error),

    /// Custom error
    #[error("custom error: {0}")]
    Custom(String),
}

/// Convenience type for Results
pub type Result<T> = std::result::Result<T, Error>;
