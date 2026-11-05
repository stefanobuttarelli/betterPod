# Local Rust experiment only. The stable Go release uses tools/Dockerfile.
FROM betterpod-build:1
ARG RUST_VERSION=1.98.1
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo
ENV PATH=/opt/cargo/bin:$PATH
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl build-essential pkg-config xvfb xauth xdotool x11-utils libegl1-mesa-dev libgles2-mesa-dev libgl1-mesa-dri \
 && rm -rf /var/lib/apt/lists/* \
 && curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o /tmp/rustup.sh \
 && sh /tmp/rustup.sh -y --no-modify-path --profile minimal --default-toolchain "$RUST_VERSION" \
 && rustup target add aarch64-unknown-linux-gnu \
 && rm /tmp/rustup.sh

# Rust 1.98 enables the Cortex-A53 843419 linker workaround. Zig 0.13 cannot
# forward it, so retain GNU ld and provide the console's older libc as sysroot.
# These are extracted, not installed over the build container's system libc.
RUN if ! command -v aarch64-linux-gnu-gcc >/dev/null; then apt-get update && apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu && rm -rf /var/lib/apt/lists/*; fi \
 && curl -fL --retry 2 'https://archive.debian.org/debian/pool/main/g/glibc/libc6_2.28-10+deb10u1_arm64.deb' -o /tmp/libc6.deb \
 && curl -fL --retry 2 'https://archive.debian.org/debian/pool/main/g/glibc/libc6-dev_2.28-10+deb10u1_arm64.deb' -o /tmp/libc6-dev.deb \
 && echo '09f5e92b1527cdc06b757499bb4d97ddf1b064fc3d163f978e5de18b131cd330  /tmp/libc6.deb' | sha256sum -c - \
 && echo '04097716018288cef8b82339faa1a9847838b89f39574b5b169f4657da8bf269  /tmp/libc6-dev.deb' | sha256sum -c - \
 && mkdir -p /opt/knulli-glibc228 \
 && dpkg-deb -x /tmp/libc6.deb /opt/knulli-glibc228 \
 && dpkg-deb -x /tmp/libc6-dev.deb /opt/knulli-glibc228 \
 && python3 -c 'from pathlib import Path; import os; root=Path("/opt/knulli-glibc228"); links=[p for p in root.rglob("*") if p.is_symlink() and os.readlink(p).startswith("/")]; [(lambda p,t:(p.unlink(),p.symlink_to(os.path.relpath(root / t.lstrip("/"),p.parent))))(p,os.readlink(p)) for p in links]' \
 && rm /tmp/libc6.deb /tmp/libc6-dev.deb
