use crate::config::Config;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::MissedTickBehavior;

/// An update downloads tens of megabytes from GitHub, so it isn't quick, but the first one runs
/// before the bot connects. Give up rather than never coming online at all.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(300);

/// Updates yt-dlp in place and logs the outcome.
///
/// Never fatal: an install that can't self-update, or a machine that can't reach GitHub right now,
/// keeps playing with the version it already has.
pub async fn update_ytdl_now(config: &Config) {
    if !config.ytdl.update.enabled {
        return;
    }

    log::debug!(
        "Checking for a youtube-dl update from {}",
        config.ytdl.update.channel
    );
    let update = mrvn_back_ytdl::update_ytdl(&config.ytdl.name, &config.ytdl.update.channel);
    match tokio::time::timeout(UPDATE_TIMEOUT, update).await {
        Ok(Ok(result)) => log::info!("youtube-dl update: {}", result),
        Ok(Err(why)) => log::warn!("Unable to update youtube-dl: {}", why),
        Err(_) => log::warn!(
            "Gave up updating youtube-dl after {} seconds",
            UPDATE_TIMEOUT.as_secs()
        ),
    }
}

pub async fn update_loop(config: Arc<Config>) -> ! {
    let interval_secs = config.ytdl.update.check_interval_secs;

    // A zero interval means "only update at startup", which already happened. This future is
    // joined with the rest of the bot, so it has to keep the shape of a loop that never ends.
    if !config.ytdl.update.enabled || interval_secs == 0 {
        futures::future::pending::<()>().await;
        unreachable!()
    }

    let period = Duration::from_secs(interval_secs);
    // The startup update covers the first tick, so wait a full period before checking again.
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    // Updates replace the binary in place, so they run one at a time. If one takes longer than
    // the interval, space the next one out rather than starting it immediately.
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        interval.tick().await;
        update_ytdl_now(&config).await;
    }
}
