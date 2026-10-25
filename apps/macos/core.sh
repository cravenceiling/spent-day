# Sourced by build.sh and test.sh. `build_core <dir>` builds the core for the app's
# target and writes its Swift bindings to <dir>. It sets `core_flags` to the swiftc
# arguments that compile the bindings and link the core.

build_core() {
  local generated="$1"
  local root target cargo_target
  root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
  target=aarch64-apple-darwin
  cargo_target="${CARGO_TARGET_DIR:-$root/target}"
  # Cargo runs from the repo root, so a relative target dir means root-relative for swiftc too.
  case "$cargo_target" in /*) ;; *) cargo_target="$root/$cargo_target" ;; esac

  # The C code in the core's dependencies (SQLite, ring) must target the same minimum macOS as the app.
  export MACOSX_DEPLOYMENT_TARGET=14.0
  (cd "$root" && cargo build --locked -p spentday-core --release --target "$target")

  rm -rf "$generated"
  (cd "$root" && cargo run --locked -q -p spentday-core --features bindgen --bin uniffi-bindgen -- \
    generate --library "$cargo_target/$target/release/libspentday_core.dylib" \
    --language swift --out-dir "$generated")

  # The static lib is passed by path: -lspentday_core would pick the dylib next to it.
  core_flags=(
    -Xcc "-fmodule-map-file=$generated/spentday_coreFFI.modulemap" -I "$generated"
    "$generated/spentday_core.swift"
    "$cargo_target/$target/release/libspentday_core.a"
    -framework Security -framework SystemConfiguration -framework CoreFoundation -liconv -lresolv
  )
}
