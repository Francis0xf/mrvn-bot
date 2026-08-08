#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Parse(serde_json::Error, String),
    Ytdl(String),
    Http(reqwest::Error),
    SongbirdJoin(songbird::error::JoinError),
    SongbirdControl(songbird::error::ControlError),
    UnsupportedUrl,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Io(err) => err.fmt(f),
            Error::Parse(err, value) => write!(f, "{}: {}", err, value),
            Error::Ytdl(err) => write!(f, "Could not load media: {}", err),
            Error::Http(err) => err.fmt(f),
            Error::SongbirdJoin(err) => err.fmt(f),
            Error::SongbirdControl(err) => err.fmt(f),
            Error::UnsupportedUrl => write!(f, "Unsupported URL"),
        }
    }
}

impl std::error::Error for Error {}
