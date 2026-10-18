FROM rust:1.99.0-bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6

# Fix package resolution to one immutable Debian archive date.
RUN rm -f /etc/apt/sources.list /etc/apt/sources.list.d/debian.sources \
    && printf '%s\n' \
      'deb [check-valid-until=no] https://snapshot.debian.org/archive/debian/20261001T000000Z/ bookworm main' \
      'deb [check-valid-until=no] https://snapshot.debian.org/archive/debian-security/20261001T000000Z/ bookworm-security main' \
      > /etc/apt/sources.list \
    && apt-get update \
    && apt-get install --no-install-recommends -y cmake clang libclang-dev pkg-config \
    && rm -rf /var/lib/apt/lists/* \
    && rustup component add rustfmt clippy \
    && dpkg-query -W > /usr/local/share/remora-build-packages.txt

# Phase 2 fixture prerequisites already supplied by the pinned image/packages.
RUN python3 --version && openssl version

WORKDIR /work
# Source/lockfile are mounted at runtime; the image contains build tools only.
CMD ["cargo", "build", "--locked", "--release"]
