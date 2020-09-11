FROM osig/rust-ubuntu:1.44.1 AS builder
COPY Cargo.toml /Cargo.toml
COPY Cargo.lock /Cargo.lock
COPY hawkeye-api /hawkeye-api
COPY hawkeye-core /hawkeye-core
COPY hawkeye-worker /hawkeye-worker
RUN cargo build --release --package hawkeye-api

FROM ubuntu:18.04 AS app
RUN apt update \
    && apt install -y \
           libssl1.0.0 \
           libssl-dev \
    && apt-get clean
ENV RUST_LOG=info
COPY --from=builder /target/release/hawkeye-api .
ENTRYPOINT ["/hawkeye-api"]
