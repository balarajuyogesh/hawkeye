FROM alpine:3.12.0 AS builder

RUN echo http://dl-cdn.alpinelinux.org/alpine/edge/testing >> /etc/apk/repositories
RUN apk update
RUN apk add --no-cache \
    automake \
    bison \
    autoconf \
    gettext-dev \
    git \
    gcc \
    meson \
    musl-dev \
    rust \
    cargo \
    glib \
    build-base \
    flex \
    gnutls-dev \
    gtk-doc \
    libffi-dev \
    libmount \
    libsrtp-dev \
    libtool \
    python3 \
    libvpx-dev \
    linux-headers \
    openssl-dev \
    opus-dev \
    x264-dev \
    cmake \
    zlib-dev \
    ffmpeg \
    ffmpeg-libs

RUN wget https://gstreamer.freedesktop.org/src/gst-libav/gst-libav-1.17.90.tar.xz \
    && tar xvfJ gst-libav-1.17.90.tar.xz > /dev/null \
    && cd gst-libav-1.17.90 \
    && meson build \
    && ninja -C build install \
    && cd /

RUN wget https://gstreamer.freedesktop.org/src/gstreamer/gstreamer-1.17.90.tar.xz \
    && tar xvfJ gstreamer-1.17.90.tar.xz > /dev/null \
    && cd gstreamer-1.17.90 \
    && meson build \
    && ninja -C build install \
    && cd /

# gst-plugins-base
RUN wget https://gstreamer.freedesktop.org/src/gst-plugins-base/gst-plugins-base-1.17.90.tar.xz \
    && tar xvfJ gst-plugins-base-1.17.90.tar.xz > /dev/null \
    && cd gst-plugins-base-1.17.90 \
    && meson build \
    && ninja -C build install \
    && cd /

# libnice
RUN git clone https://github.com/libnice/libnice.git \
    && cd libnice \
    && meson builddir \
    && ninja -C builddir \
    && ninja -C builddir install \
    && cd

# gst-plugins-good
RUN wget https://gstreamer.freedesktop.org/src/gst-plugins-good/gst-plugins-good-1.17.90.tar.xz \
    && tar xvfJ gst-plugins-good-1.17.90.tar.xz > /dev/null \
    && cd gst-plugins-good-1.17.90 \
    && meson build \
    && ninja -C build install \
    && cd

# gst-plugins-bad
RUN wget https://gstreamer.freedesktop.org/src/gst-plugins-bad/gst-plugins-bad-1.17.90.tar.xz \
    && tar xvfJ gst-plugins-bad-1.17.90.tar.xz > /dev/null \
    && cd gst-plugins-bad-1.17.90 \
    && meson build \
    && ninja -C build install \
    && cd /

# gst-rtsp-server
RUN wget https://gstreamer.freedesktop.org/src/gst-rtsp-server/gst-rtsp-server-1.17.90.tar.xz \
    && tar xvfJ gst-rtsp-server-1.17.90.tar.xz > /dev/null \
    && cd gst-rtsp-server-1.17.90 \
    && meson build \
    && ninja -C build install \
    && cd / \
    rm -rf gst*

COPY Cargo.toml /Cargo.toml
COPY Cargo.lock /Cargo.lock
COPY src /src
RUN cargo build --release
ENTRYPOINT ["/target/release/video-slate-detector"]
