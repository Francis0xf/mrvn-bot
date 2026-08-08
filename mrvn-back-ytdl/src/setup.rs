use crate::PlayConfig;
use std::fmt::{Debug, Display, Formatter};
use std::io::{Error, Result};
use std::process::ExitStatus;
use tokio::process::Command;

#[derive(Debug)]
pub struct StatusCodeError(ExitStatus);

impl Display for StatusCodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "status code {}", self.0)
    }
}

impl std::error::Error for StatusCodeError {}

/// yt-dlp reads a bare `--update-to` value as a tag unless it names one of its own channels
/// (`stable`, `nightly`, `master`), so a repository used as a mirror needs an explicit tag to
/// resolve to anything.
fn normalize_update_target(update_to: &str) -> String {
    if update_to.contains('/') && !update_to.contains('@') {
        format!("{}@latest", update_to)
    } else {
        update_to.to_owned()
    }
}

/// Runs yt-dlp's built-in self-updater, replacing the binary in place. `update_to` is a
/// `--update-to` target: a channel, a `channel@tag` pair, or a GitHub repository to pull builds
/// from instead of the official one.
///
/// Only the standalone binaries can do this. Installs from pip or a distro package manager fail
/// here, and are expected to be updated by whatever installed them.
pub async fn update_ytdl(ytdl_name: &str, update_to: &str) -> Result<String> {
    let ytdl = Command::new(ytdl_name)
        .arg("--update-to")
        .arg(normalize_update_target(update_to))
        // Without this an update that outlives the timeout below keeps running, and could still
        // swap the binary out from under a later attempt.
        .kill_on_drop(true)
        .output()
        .await?;

    if !ytdl.status.success() {
        // yt-dlp explains itself on stderr, and the exit status alone doesn't distinguish
        // "no network" from "this install can't self-update".
        let reason = String::from_utf8_lossy(&ytdl.stderr);
        let reason = reason.trim();
        return Err(Error::other(if reason.is_empty() {
            format!("{}", ytdl.status)
        } else {
            format!("{}: {}", ytdl.status, reason)
        }));
    }

    // The updater narrates each step it takes, and only the last line says how it ended up.
    let stdout = String::from_utf8_lossy(&ytdl.stdout);
    Ok(stdout
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or("no output")
        .to_owned())
}

pub async fn get_ytdl_version(config: &PlayConfig<'_>) -> Result<String> {
    let ytdl = Command::new(config.ytdl_name)
        .arg("--version")
        .output()
        .await?;

    if ytdl.status.success() {
        match String::from_utf8(ytdl.stdout) {
            Ok(mut version_raw) => {
                // remove any trailing whitespace (probably a newline)
                version_raw.truncate(version_raw.trim_end().len());
                Ok(version_raw)
            }
            Err(err) => Err(Error::other(err)),
        }
    } else {
        Err(Error::other(StatusCodeError(ytdl.status)))
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_update_target;

    #[test]
    fn leaves_channel_names_alone() {
        assert_eq!(normalize_update_target("stable"), "stable");
        assert_eq!(normalize_update_target("nightly"), "nightly");
    }

    #[test]
    fn leaves_explicit_tags_alone() {
        assert_eq!(
            normalize_update_target("stable@2026.07.04"),
            "stable@2026.07.04"
        );
        assert_eq!(
            normalize_update_target("my-org/yt-dlp@2026.07.04"),
            "my-org/yt-dlp@2026.07.04"
        );
    }

    #[test]
    fn resolves_a_bare_repository_to_its_latest_release() {
        assert_eq!(
            normalize_update_target("my-org/yt-dlp"),
            "my-org/yt-dlp@latest"
        );
    }
}
