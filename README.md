# ![MRVN smiley face](mrvn.png) MRVN

MRVN is a Discord music player bot. It has a few neat features:

 - Supports a wide array of sites, including Youtube, Soundcloud, Twitch and
   [many more](https://ytdl-org.github.io/youtube-dl/supportedsites.html).
   Sites can be blocked, and the default search site can be configured.
 - Exclusively uses Discord application commands.
 - Multi-channel support: allows simultaneous playback in multiple channels by
   using multiple bot applications.
 - Per-user queues: your queued songs follow you between channels. Each bot
   alternates between songs queued by people in the channel, so nobody misses
   out.

## Commands

 - `/play [query or url]` adds a song to your queue and starts playback in the
   channel if required.
 - `/pause` pauses the current song playing your voice channel.
 - `/resume` unpauses the current song, or makes the bot start playing if you
   have previously queued songs.
 - `/skip` skips the current song, or votes to skip if it you weren't the
   original queue-er. The number of votes needed is configurable.
 - `/stop` skips the current song and doesn't play any more queued songs. Use
   `/resume` to continue playback.
 - `/replace` replaces your most recently queued song.
 - `/nowplaying` shows the current song and how far through it you are.
 - Queue management is not implemented yet.

## Set up

MRVN is self-hosted. This means you must register your own Discord applications
and run the bot on your own system. It's written in
[Rust](https://www.rust-lang.org/) and runs on Windows, Linux and macOS. You
can build MRVN yourself or use our prebuilt Docker images, but either way you
must set up the Discord application first:

### Creating the Discord Applications

1. Open the [Discord Developer Portal](https://discord.com/developers) and
   create an application for each channel you want to be able to play
   simultaneously. E.g. if your guild has three voice channels, you might want
   three applications to be able to listen to music in all channels at the same
   time.
2. Ensure you create a Bot user for each application. You can do this from the
   "Bot" panel in the application settings.
3. Download a copy of the [config.example.json](https://github.com/cpdt/mrvn-bot/blob/master/config.example.json)
   file and save it somewhere, maybe as config.json. This file contains your 
   configuration for the bot, including Discord application tokens.
4. Open the new config.json file and add the bot token and application ID for
   each Discord application. The "command bot" is the one that has application
   commands registered against it. It can be one of the voice bots, but you must
   also include it in the voice bot list.
5. Add each bot user to your Discord guild:
    - Visit the following URL to add the command bot, replacing
      `APPLICATION_ID_HERE` with the bots application ID:
      `https://discord.com/oauth2/authorize?client_id=APPLICATION_ID_HERE&scope=bot%20applications.commands&permissions=3145728`
    - Visit the following URL to add each non-command bot, again replacing
      `APPLICATION_ID_HERE` with the bots application ID:
      `https://discord.com/oauth2/authorize?client_id=APPLICATION_ID_HERE&scope=bot&permissions=3145728`
    - The different between these is because the command bot needs to request
      extra permissions to create application commands.

### Run the Docker image (recommended)

Before running, ensure you have [the Docker Engine installed](https://docs.docker.com/engine/install/).

Once that's done, you can run the following command any time you want to start MRVN. Make sure to replace `/path/to/config.json` with the path to your configuration file saved previously.

```
docker run --name mrvn-bot --rm --mount type=bind,source=/path/to/config.json,target=/config.json ghcr.io/cpdt/mrvn-bot:latest
```

You can stop MRVN by running `docker stop mrvn-bot`.

### Build and run locally

This is an alternative to running the Docker image as described above. I would
recommend you follow those instructions as they involve less setting up, but
you're welcome to build and run MRVN yourself.

First off you need to setup your environment:

 1. Ensure you have the required dependencies installed:
    - [Git](https://git-scm.com/)
    - [Rustup](https://rustup.rs/)
    - [yt-dlp](https://github.com/yt-dlp/yt-dlp), plus a JavaScript runtime such
      as [Deno](https://deno.com/) for sites that need one. The `ytdl.name`
      field in your config file must match the name of the binary — the example
      config says `youtube-dl`, which is the name the Docker image installs
      yt-dlp under. Install one of
      [the standalone builds](https://github.com/yt-dlp/yt-dlp/releases) if you
      want MRVN to keep yt-dlp up to date for you, as described below.
 2. Clone the repository by running `git clone https://github.com/cpdt/mrvn-bot`

Once that's done, you can run the following command from inside the repository any time you want to start MRVN. Make sure to replace `/path/to/config.json` with the path to your configuration file saved previously.

```
cargo run --release /path/to/config.json
```

The first time this runs it will build MRVN, which can take a while. After it's been built once it should start immediately.

If you want to see logging output, set the `RUST_LOG` environment variable to `mrvn` before running the above command. This uses [the syntax from the env-logger library](https://docs.rs/env_logger/0.9.0/env_logger/index.html#enabling-logging).

You can stop MRVN by pressing Ctrl+C in the terminal window.

## Keeping yt-dlp up to date

Sites change how they serve audio far more often than MRVN releases, and a
yt-dlp that is a few weeks old will eventually stop resolving songs. Rather than
tying that to how recently you pulled a new image, MRVN runs yt-dlp's own
updater: once at startup, then on an interval. Failures are logged and ignored,
so a machine that can't reach GitHub keeps playing with the version it has.

This is configured under `ytdl.update` in your config file:

```json
"ytdl": {
  "name": "youtube-dl",
  "update": {
    "enabled": true,
    "channel": "stable",
    "check_interval_secs": 21600
  }
}
```

 - `enabled` turns updates off if you would rather manage yt-dlp yourself. It
   defaults to `true`, so a config written before this option existed gets
   updates.
 - `channel` is passed to yt-dlp as `--update-to`. As well as the `stable`,
   `nightly` and `master` channels it accepts a specific version
   (`stable@2026.07.04`), or a GitHub repository to pull builds from instead of
   the official one (`my-org/yt-dlp`), which is useful if you mirror releases
   or run a fork.
 - `check_interval_secs` is how long to wait between checks. Set it to `0` to
   only update at startup.

Only the standalone yt-dlp binaries can update themselves, which is what the
Docker image installs. If you installed yt-dlp with pip or a distro package
manager, updates will fail with a warning on every check — use whatever
installed it, and set `enabled` to `false`.

Updates in the Docker image are written into the container, so recreating it
starts again from the version baked into the image. That's fine, it just means
each fresh container downloads yt-dlp once on startup.

## Why?

In mid-2021 [Groovy](https://groovy.bot) and [Rythm](https://rythm.fm), Discord’s two largest music bots, were taken offline by YouTube. In the wake of this, I created MRVN mainly to serve a couple of servers I’m in, but also as an open tool for anyone looking for a new music bot.

Unlike Groovy, Rythm and many like them, MRVN was designed from the ground up to be self-hosted and simple to setup, making it impossible to be taken down as a whole. As open source software MRVN also does not charge its users, following the most notable clause in YouTube’s terms of service that commercial music bots break.

Finally, as a personal project MRVN has been an opportunity for me to re-think how my friends and I use music bots. This has led to what I consider improvements over the old formula: playing songs in a round-robin pattern so everybody gets a go, handling servers with multiple voice channels with a breeze, and codifying the unspoken “you shall not skip a song that is not yours” rule.

Please reach out and let me know if you’re using MRVN on your server! I would love to hear what works and what doesn’t, and see how it’s being used “in the wild”.

## License

MRVN is available under the [MIT license](https://opensource.org/licenses/MIT).
See the LICENSE file for details.

The MRVN smiley face used in this document is sourced from the [Titanfall Wiki](https://titanfall.fandom.com/wiki/Mk._III_Mobile_Robotic_Versatile_Entity_Automated_Assistant) and is copyright Respawn Entertainment 2014.
