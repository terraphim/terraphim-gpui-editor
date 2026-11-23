#!/usr/bin/env bash
set -euo pipefail

bundler=$1
binary=$2
output=$3

: "${OMA_RELEASE_SEED_BASE64:?Set the release seed secret}"
: "${OMA_PUBLISHER:?Set the publisher hostname}"
: "${OMA_ROOT_KEY_ID:?Set the publisher root key ID}"
: "${OMA_UPDATE_URL:?Set the HTTPS release URL}"
: "${OMA_RELEASE_NUMBER:?Set the monotonic release number}"
[[ $OMA_PUBLISHER =~ ^[a-z0-9]+([.-][a-z0-9]+)*$ && $OMA_PUBLISHER == *.* && ${#OMA_PUBLISHER} -le 253 ]]
[[ $OMA_ROOT_KEY_ID =~ ^[0-9a-f]{16}$ ]]
[[ $OMA_RELEASE_NUMBER =~ ^[1-9][0-9]*$ ]]
[[ $OMA_UPDATE_URL == "https://$OMA_PUBLISHER/"* && $OMA_UPDATE_URL == *.oma ]]

umask 077
stage=$(mktemp -d "${RUNNER_TEMP:-/tmp}/terraphim-oma.XXXXXX")
trap 'rm -rf -- "$stage"' EXIT
printf '%s' "$OMA_RELEASE_SEED_BASE64" | openssl base64 -d -A > "$stage/release.seed"
unset OMA_RELEASE_SEED_BASE64
[[ $(wc -c < "$stage/release.seed") -eq 32 ]]

manifest=$(dirname -- "$0")/manifest.toml
{
    printf 'publisher = "%s"\n' "$OMA_PUBLISHER"
    cat "$manifest"
} > "$stage/manifest.toml"

test ! -e "$output"
mkdir -p "$output"
"$bundler" bundle "$binary" --manifest "$stage/manifest.toml" -o "$output" \
    --sign "$stage/release.seed" --root "$OMA_ROOT_KEY_ID" \
    --release "$OMA_RELEASE_NUMBER" --update-url "$OMA_UPDATE_URL"
(
    cd "$output"
    shopt -s nullglob
    bundles=(*.oma)
    [[ ${#bundles[@]} -eq 1 && -f ${bundles[0]} ]]
    sha256sum -- "${bundles[@]}" > SHA256SUMS
)
