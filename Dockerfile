FROM rust:1.94-bookworm AS builder
# cmake is needed to build aws-lc-rs, which rustls pulls in through reqwest.
RUN apt-get update && apt-get install -y cmake
WORKDIR /usr/src/mrvn-bot
COPY . .
RUN cargo install --path ./mrvn-front-discord

# The bot updates yt-dlp itself on startup and on an interval (see `ytdl.update` in the config),
# so this version is only a floor: it's what a container falls back to when it can't reach GitHub.
FROM debian:stable-slim AS ytdl
ARG YTDLP_VERSION=2026.07.04
RUN apt-get update && apt-get install -y ca-certificates curl
RUN curl -fL https://github.com/yt-dlp/yt-dlp/releases/download/${YTDLP_VERSION}/yt-dlp_linux -o /youtube-dl && chmod a+rx /youtube-dl

FROM debian:stable-slim
RUN apt-get update && apt-get install -y ca-certificates ffmpeg
RUN update-ca-certificates
COPY --from=ytdl /youtube-dl /usr/local/bin/youtube-dl
# Deno publishes a binary-only image for exactly this, so there's nothing to download or unpack.
COPY --from=denoland/deno:bin-2.9.5 /deno /usr/local/bin/deno
COPY --from=builder /usr/local/cargo/bin/mrvn-front-discord /usr/local/bin/mrvn-front-discord
ENV RUST_LOG=mrvn
CMD ["mrvn-front-discord", "config.json"]
LABEL org.opencontainers.image.source="https://github.com/cpdt/mrvn-bot"
