#!/usr/bin/env bash
# Builds libspentday_core.so for the emulator and device ABIs and generates the Kotlin bindings.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
jni_libs="$here/app/src/main/jniLibs"
bindings="${1:-$here/app/build/generated/uniffi/kotlin}"

export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-${ANDROID_HOME:?set ANDROID_HOME}/ndk/28.2.13676358}"

cd "$root"
cargo ndk -t arm64-v8a -t x86_64 -o "$jni_libs" build -p spentday-core --release
cargo build -p spentday-core --release

host_lib="$root/target/release/libspentday_core.dylib"
[[ -f "$host_lib" ]] || host_lib="$root/target/release/libspentday_core.so"

rm -rf "$bindings"
cargo run -q -p spentday-core --features bindgen --bin uniffi-bindgen -- \
  generate --library "$host_lib" --config "$here/uniffi.toml" --language kotlin --no-format --out-dir "$bindings"
