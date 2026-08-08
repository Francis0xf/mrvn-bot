mod brain;
mod error;
mod formats;
mod input;
mod setup;
mod song;
mod songbird;
mod speaker;

pub use self::brain::*;
pub use self::error::*;
pub use self::setup::*;
pub use self::song::*;
pub use self::speaker::*;

lazy_static::lazy_static! {
    static ref HTTP_CLIENT: reqwest::Client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        // Per-read, not per-request: a request here streams a whole song, but a remote that
        // accepts the connection and then stalls should not hang playback forever.
        .read_timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("Unable to build HTTP client");
}
