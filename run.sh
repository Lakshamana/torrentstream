#!/bin/sh

exec cargo run \
  --quiet \
  --release \
  --target-dir=/tmp/torrentstream \
  --manifest-path $(dirname "$0")/Cargo.toml -- "$@"
