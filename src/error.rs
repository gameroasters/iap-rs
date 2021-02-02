use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("serde_json error: {0}")]
    SerdeError(#[from] serde_json::Error),

    #[error("http error: {0}")]
    HttpError(#[from] hyper::http::Error),

    #[error("hyper error: {0}")]
    HyperError(#[from] hyper::Error),

    #[error("yup_oauth error: {0}")]
    YupOauth2Error(#[from] yup_oauth2::Error),

    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("parse int error: {0}")]
    ParseIntError(#[from] std::num::ParseIntError),

    #[error("utf8 error: {0}")]
    FromUtf8Error(#[from] std::string::FromUtf8Error)
}

pub type Result<T> = std::result::Result<T, Error>;
