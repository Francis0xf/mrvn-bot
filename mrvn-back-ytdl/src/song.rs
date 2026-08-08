use crate::input::{hls_chunks, remote_file_chunks};
use crate::{Error, HTTP_CLIENT};
use futures::{TryStreamExt, future};
use serenity::async_trait;
use serenity::model::prelude::UserId;
use songbird::input::core::io::MediaSource;
use songbird::input::{AsyncAdapterStream, AsyncMediaSource, AudioStream, Input, LiveInput};
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::SeekFrom;
use std::pin::Pin;
use std::process::Stdio;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncSeek, BufReader, ReadBuf};
use tokio::process::Command as TokioCommand;
use tokio_util::io::StreamReader;
use url::Url;
use uuid::Uuid;

pub struct Song {
    pub metadata: SongMetadata,
    download_url: String,
    http_headers: Vec<(String, String)>,
}

pub struct PlayConfig<'s> {
    pub search_prefix: &'s str,
    pub host_blocklist: &'s [String],
    pub ytdl_name: &'s str,
    pub ytdl_args: &'s [String],
    pub buffer_capacity_kb: usize,
}

#[derive(serde::Deserialize)]
struct YtdlOutput {
    pub title: String,
    pub fulltitle: Option<String>,
    pub description: Option<String>,
    pub extractor: String,
    pub webpage_url: String,
    pub url: String,
    pub thumbnail: Option<String>,
    pub http_headers: HashMap<String, String>,
    pub duration: Option<f64>,
}

fn parse_ytdl_line(line: &str, user_id: UserId) -> Result<Song, Error> {
    let trimmed_line = line.trim();
    if let Some(error) = trimmed_line.strip_prefix("ERROR: ") {
        return Err(Error::Ytdl(error.to_string()));
    }

    let value: YtdlOutput = serde_json::from_str(trimmed_line)
        .map_err(|err| Error::Parse(err, trimmed_line.to_string()))?;

    // Twitch stream extractor puts the stream title as the description for some reason
    let title = match &value.extractor as &str {
        "twitch:stream" => value.description,
        _ => value.fulltitle,
    };
    let title = title.unwrap_or(value.title);

    Ok(Song {
        metadata: SongMetadata {
            id: Uuid::new_v4(),
            title,
            url: value.webpage_url,
            thumbnail_url: value.thumbnail,
            duration_seconds: if value.duration == Some(0.) {
                None
            } else {
                value.duration
            },
            user_id,
        },
        download_url: value.url.to_string(),
        http_headers: value
            .http_headers
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
    })
}

/// Rewrites YouTube links into a canonical form, dropping tracking and playlist-position
/// parameters that can confuse youtube-dl. Returns `None` for any link we don't know how to
/// rewrite, in which case the original URL should be passed through untouched.
fn normalize_youtube_url(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    let host = host.strip_prefix("www.").unwrap_or(host);

    let (path, video_id) = match host {
        "youtube.com" | "m.youtube.com" | "music.youtube.com" => {
            let mut video_id = None;
            let mut playlist_id = None;
            for (key, value) in url.query_pairs() {
                match &*key {
                    "v" if video_id.is_none() => video_id = Some(value.into_owned()),
                    "list" if playlist_id.is_none() => playlist_id = Some(value.into_owned()),
                    _ => {}
                }
            }

            // A link to a video within a playlist plays just that video, matching what a user
            // sees when they open the link themselves.
            match (video_id, playlist_id) {
                (Some(video_id), _) => ("watch", ("v", video_id)),
                (None, Some(playlist_id)) => ("playlist", ("list", playlist_id)),
                // Channel pages, /shorts, /live and friends have no parameters worth keeping.
                (None, None) => return None,
            }
        }
        "youtu.be" => {
            // Unlike query_pairs(), path() hands back percent-encoded text, so it has to be
            // decoded here or it gets encoded a second time below.
            let video_id = url.path().trim_matches('/');
            if video_id.is_empty() {
                return None;
            }
            let video_id = percent_encoding::percent_decode_str(video_id)
                .decode_utf8_lossy()
                .into_owned();
            ("watch", ("v", video_id))
        }
        _ => return None,
    };

    let mut normalized = Url::parse("https://www.youtube.com/").ok()?;
    normalized.set_path(path);
    normalized
        .query_pairs_mut()
        .append_pair(video_id.0, &video_id.1);
    Some(normalized.into())
}

impl Song {
    pub async fn load(
        term: &str,
        user_id: UserId,
        config: &PlayConfig<'_>,
    ) -> Result<Vec<Song>, Error> {
        let ytdl_url = match url::Url::parse(term) {
            Ok(url) => {
                if let Some(host_str) = url.host_str() {
                    // Ensure the resolved host isn't in the blocklist
                    if config
                        .host_blocklist
                        .iter()
                        .any(|domain| host_str.contains(domain))
                    {
                        return Err(Error::UnsupportedUrl);
                    }
                }

                match normalize_youtube_url(&url) {
                    Some(normalized) => Cow::Owned(normalized),
                    None => Cow::Borrowed(term),
                }
            }
            Err(_) => Cow::Owned(format!("{}:{}", config.search_prefix, &term)),
        };

        let mut ytdl = TokioCommand::new(config.ytdl_name)
            .args(config.ytdl_args)
            .args([
                "--dump-json",
                "--ignore-config",
                "--no-warnings",
                ytdl_url.as_ref(),
                "-o",
                "-",
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .stdout(Stdio::null())
            // Without this a youtube-dl process outlives an early return, writing to a pipe
            // nobody is reading from.
            .kill_on_drop(true)
            .spawn()
            .map_err(Error::Io)?;
        let mut lines = BufReader::new(ytdl.stderr.take().unwrap()).lines();

        // youtube-dl writes one JSON object per resolved song, and reports per-song problems
        // inline. A single unusable entry (a region-locked video in a playlist, say) shouldn't
        // lose the songs that did resolve, so errors are only reported if nothing resolved.
        let mut songs = Vec::new();
        let mut last_error = None;
        while let Some(line) = lines.next_line().await.map_err(Error::Io)? {
            if line.trim().is_empty() {
                continue;
            }
            match parse_ytdl_line(&line, user_id) {
                Ok(song) => songs.push(song),
                Err(why) => {
                    log::warn!("Ignoring unusable youtube-dl output: {}", why);
                    last_error = Some(why);
                }
            }
        }

        let status = ytdl.wait().await.map_err(Error::Io)?;
        if songs.is_empty() {
            if let Some(why) = last_error {
                return Err(why);
            }
            if !status.success() {
                return Err(Error::Ytdl(format!("youtube-dl exited with {}", status)));
            }
        }

        Ok(songs)
    }

    pub async fn fetch_one(
        webpage_url: &str,
        user_id: UserId,
        config: &PlayConfig<'_>,
    ) -> Result<Song, Error> {
        let mut ytdl = TokioCommand::new(config.ytdl_name)
            .args(config.ytdl_args)
            .args([
                "--dump-json",
                "--ignore-config",
                "--no-warnings",
                "--no-playlist",
                webpage_url,
                "-o",
                "-",
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .stdout(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(Error::Io)?;
        let first_line = BufReader::new(ytdl.stderr.take().unwrap())
            .lines()
            .next_line()
            .await
            .map_err(Error::Io)?
            .ok_or(Error::UnsupportedUrl)?;

        parse_ytdl_line(&first_line, user_id)
    }

    pub async fn get_input(
        &self,
        config: &PlayConfig<'_>,
    ) -> Result<songbird::input::Input, Error> {
        // The cached download URL might have become invalid since fetching it. We assume it's fine
        // but fetch a new one from youtube-dl if playback fails.
        match self.get_input_no_retry(config).await {
            Ok(input) => Ok(input),
            Err(why) => {
                log::error!(
                    "Error opening stream to play {}: {}",
                    &self.metadata.url,
                    why
                );
                let refetch_song =
                    Song::fetch_one(&self.metadata.url, self.metadata.user_id, config).await?;
                refetch_song.get_input_no_retry(config).await
            }
        }
    }

    async fn get_input_no_retry(
        &self,
        config: &PlayConfig<'_>,
    ) -> Result<songbird::input::Input, Error> {
        let parsed_download_url =
            url::Url::parse(&self.download_url).map_err(|_| Error::UnsupportedUrl)?;

        // Start streaming data from the remote. Headers come from whatever extractor youtube-dl
        // used, so an unusable one is skipped rather than failing the whole play.
        let mut headers = reqwest::header::HeaderMap::new();
        for (key, value) in &self.http_headers {
            let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                value.parse(),
            ) else {
                log::warn!("Ignoring unusable header {:?} from youtube-dl", key);
                continue;
            };
            headers.insert(name, value);
        }

        let request_builder = HTTP_CLIENT.get(&self.download_url).headers(headers.clone());
        create_source(config, parsed_download_url, headers, request_builder).await
    }
}

#[derive(Clone)]
pub struct SongMetadata {
    pub id: Uuid,
    pub title: String,
    pub url: String,
    pub thumbnail_url: Option<String>,
    pub duration_seconds: Option<f64>,
    pub user_id: UserId,
}

async fn create_source(
    config: &PlayConfig<'_>,
    request_url: url::Url,
    headers: reqwest::header::HeaderMap,
    request_builder: reqwest::RequestBuilder,
) -> Result<Input, Error> {
    let buffer_capacity_bytes = config.buffer_capacity_kb * 1024;

    let initial_response = request_builder
        .try_clone()
        .unwrap()
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(Error::Http)?;

    let maybe_extension = request_url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .and_then(|segment| segment.rfind('.').map(|idx| (segment, idx)))
        .map(|(segment, idx)| &segment[(idx + 1)..]);

    let maybe_mime_type = initial_response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|val| val.to_str().ok());

    let is_mpeg_stream = maybe_extension == Some("m3u8")
        || maybe_extension == Some("m3u")
        || maybe_mime_type == Some("application/vnd.apple.mpegurl")
        || maybe_mime_type == Some("audio/mpegurl");

    // Start streaming chunks from the remote
    let adapter_stream = if is_mpeg_stream {
        let stream = hls_chunks(request_url, headers, initial_response, request_builder);
        let reader = StreamReader::new(stream.try_filter(|chunk| future::ready(!chunk.is_empty())));
        AsyncAdapterStream::new(
            Box::new(AsyncReader::new(Box::pin(reader))),
            buffer_capacity_bytes,
        )
    } else {
        let stream = remote_file_chunks(initial_response, request_builder);
        let reader = StreamReader::new(stream.try_filter(|chunk| future::ready(!chunk.is_empty())));
        AsyncAdapterStream::new(
            Box::new(AsyncReader::new(Box::pin(reader))),
            buffer_capacity_bytes,
        )
    };

    let audio_stream = AudioStream {
        input: Box::new(adapter_stream) as Box<dyn MediaSource>,
    };
    Ok(Input::Live(LiveInput::Raw(audio_stream), None))
}

struct AsyncReader<T> {
    inner: Pin<Box<T>>,
}

impl<T> AsyncReader<T> {
    fn new(inner: Pin<Box<T>>) -> Self {
        AsyncReader { inner }
    }
}

impl<T> AsyncRead for AsyncReader<T>
where
    T: AsyncRead,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.inner.as_mut().poll_read(cx, buf)
    }
}

impl<T> AsyncSeek for AsyncReader<T> {
    fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<u64>> {
        Poll::Ready(Err(std::io::ErrorKind::Unsupported.into()))
    }
}

#[async_trait]
impl<T> AsyncMediaSource for AsyncReader<T>
where
    T: AsyncRead + Send + Sync,
{
    fn is_seekable(&self) -> bool {
        false
    }

    async fn byte_len(&self) -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_youtube_url;

    fn normalize(url: &str) -> Option<String> {
        normalize_youtube_url(&url::Url::parse(url).unwrap())
    }

    #[test]
    fn keeps_the_video_id_from_a_watch_url() {
        assert_eq!(
            normalize("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string())
        );
    }

    #[test]
    fn drops_tracking_and_playlist_position_parameters() {
        assert_eq!(
            normalize("https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL123&index=4&t=30s"),
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string())
        );
    }

    #[test]
    fn finds_the_video_id_wherever_it_appears() {
        assert_eq!(
            normalize("https://www.youtube.com/watch?list=PL123&v=dQw4w9WgXcQ"),
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string())
        );
    }

    #[test]
    fn keeps_playlists_as_playlists() {
        assert_eq!(
            normalize("https://www.youtube.com/playlist?list=PL123"),
            Some("https://www.youtube.com/playlist?list=PL123".to_string())
        );
    }

    #[test]
    fn expands_short_links() {
        assert_eq!(
            normalize("https://youtu.be/dQw4w9WgXcQ"),
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            normalize("https://youtu.be/dQw4w9WgXcQ?si=abc123&t=30"),
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string())
        );
    }

    #[test]
    fn handles_mobile_and_music_subdomains() {
        assert_eq!(
            normalize("https://m.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            normalize("https://music.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string())
        );
    }

    /// These used to panic on an out-of-bounds index into the query parameters.
    #[test]
    fn passes_through_youtube_urls_without_parameters() {
        for url in [
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
            "https://www.youtube.com/live/dQw4w9WgXcQ",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
            "https://www.youtube.com/@somechannel",
            "https://www.youtube.com/@somechannel/videos",
            "https://www.youtube.com/",
            "https://www.youtube.com/watch",
            "https://www.youtube.com/watch?feature=share",
            "https://youtu.be/",
        ] {
            assert_eq!(normalize(url), None, "{}", url);
        }
    }

    #[test]
    fn leaves_other_hosts_alone() {
        for url in [
            "https://soundcloud.com/artist/track",
            "https://www.twitch.tv/somebody",
            "https://notyoutube.com/watch?v=dQw4w9WgXcQ",
            "https://example.com/youtube.com/watch?v=dQw4w9WgXcQ",
        ] {
            assert_eq!(normalize(url), None, "{}", url);
        }
    }

    #[test]
    fn escapes_ids_it_puts_back_into_a_url() {
        assert_eq!(
            normalize("https://youtu.be/not%20an%20id%26v%3Dother"),
            Some("https://www.youtube.com/watch?v=not+an+id%26v%3Dother".to_string())
        );
    }
}
