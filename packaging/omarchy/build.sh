#!/bin/sh
set -eu

if [ "$(uname -s)" != Linux ] || [ "$(uname -m)" != x86_64 ]; then
    echo 'Build this OMA executable on x86_64 Linux.' >&2
    exit 1
fi

project_dir=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$project_dir"

# The prototype ignores Cargo.lock. Generate it for a new checkout, and
# honor the existing resolution on later builds.
if [ ! -f Cargo.lock ]; then
    cargo generate-lockfile
fi

exec cargo build --release --locked --bin terraphim-gpui-editor \
    --target x86_64-unknown-linux-gnu --target-dir target
