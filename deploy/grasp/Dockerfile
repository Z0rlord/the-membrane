FROM rust:1-bookworm AS builder
RUN cargo install ngit-grasp --locked --version 1.2.0

FROM debian:bookworm-slim
RUN apt-get update -qq \
    && apt-get install -y -qq --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/ngit-grasp /usr/local/bin/ngit-grasp
EXPOSE 7334
ENTRYPOINT ["ngit-grasp", "serve"]
