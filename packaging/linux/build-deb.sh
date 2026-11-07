#!/bin/sh
set -eu
cd "$(dirname "$0")/../../apps/desktop"

# Keep GCC's LTO off for ring and SQLite; Rust's release profile still enables LLVM LTO.
export CFLAGS="${CFLAGS:-} -fno-lto"
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
cargo install cargo-deb --version 3.8.0 --locked
mkdir -p ../../dist ../../packaging/linux/build
sed 's|^ExecStart=.*|ExecStart=/usr/bin/spentday --background|' linux/data/spentday.service \
    > ../../packaging/linux/build/spentday.service
cargo deb --locked --output ../../dist/
dpkg-deb --info ../../dist/*.deb
dpkg-deb --contents ../../dist/*.deb
