FROM osig/rust-ubuntu:1.44.1 AS builder
RUN apt update
RUN apt install -y \
        libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
              gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
              gstreamer1.0-plugins-bad gstreamer1.0-libav libgstrtspserver-1.0-dev libges-1.0-dev
COPY Cargo.toml /Cargo.toml
COPY Cargo.lock /Cargo.lock
COPY src /src
RUN cargo build --release

FROM ubuntu:18.04 AS app
RUN apt update
RUN apt install -y \
              gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
              gstreamer1.0-plugins-bad gstreamer1.0-libav
ENV RUST_LOG=info
COPY --from=builder /target/release/hawkeye .
ENTRYPOINT ["/hawkeye"]
