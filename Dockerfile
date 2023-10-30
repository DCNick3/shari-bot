# syntax = docker/dockerfile:1.2

FROM bash AS get-tini

# Add Tini init-system
ENV TINI_VERSION v0.19.0
ADD https://github.com/krallin/tini/releases/download/${TINI_VERSION}/tini-static /tini
RUN chmod +x /tini

# I would have _loved_ to use muslrust, but v8 that rusty_x86 depends on doesn't really support musl that well
# we __might__ be able to build it ourselves, but that's future work ig
FROM rust:1-slim-buster as build

ENV CARGO_INCREMENTAL=0

# install python
RUN apt update && apt install -y curl && rm -rf /var/lib/apt/lists/*

WORKDIR /volume
COPY . .

RUN --mount=type=cache,target=/root/.cargo/registry --mount=type=cache,target=/volume/target \
    cargo build --locked --profile ship && \
    cp target/ship/shari-bot /volume/shari-bot

FROM debian:bookworm-slim

LABEL org.opencontainers.image.source https://github.com/DCNick3/shari-bot
EXPOSE 3000

ENV ENVIRONMENT=prod

COPY --from=get-tini /tini /tini
COPY --from=build /volume/shari-bot /shari-bot
COPY config.prod.yaml /

ENTRYPOINT ["/tini", "--", "/shari-bot"]