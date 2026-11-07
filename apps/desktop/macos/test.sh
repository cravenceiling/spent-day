#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
cargo test --manifest-path "$here/../Cargo.toml" --locked -- --include-ignored
"$here/tests/publish.sh"
"$here/tests/verify-cleanup.sh"
"$here/tests/make-dev-cert.sh"
