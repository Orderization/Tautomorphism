#!/usr/bin/env bash
set -euo pipefail
cargo build --release --jobs "$(nproc)"
