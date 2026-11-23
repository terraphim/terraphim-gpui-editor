#!/usr/bin/env bash
set -euo pipefail

binary=$1
output=$2

: "${MAC_STORE_APP_P12_BASE64:?Set the Mac App Distribution certificate secret}"
: "${MAC_STORE_APP_P12_PASSWORD:?Set its password secret}"
: "${MAC_STORE_INSTALLER_P12_BASE64:?Set the Mac Installer Distribution certificate secret}"
: "${MAC_STORE_INSTALLER_P12_PASSWORD:?Set its password secret}"
: "${MAC_STORE_PROFILE_BASE64:?Set the Mac App Store provisioning profile secret}"

test ! -e "$output"
umask 077
stage=$(mktemp -d "${RUNNER_TEMP:-/tmp}/terraphim-store.XXXXXX")
keychain="$stage/store.keychain-db"
keychain_password=$(openssl rand -hex 32)
echo "::add-mask::$keychain_password"
cleanup() {
    security delete-keychain "$keychain" >/dev/null 2>&1 || true
    rm -rf -- "$stage"
}
trap cleanup EXIT

script_dir=$(cd -- "$(dirname -- "$0")" && pwd)
version=$(sed -n 's/^version = "\([0-9][0-9.]*\)"$/\1/p' "$script_dir/../../Cargo.toml" | head -n 1)
app="$stage/Terraphim Editor.app"
bash "$script_dir/prepare-store.sh" "$binary" "$app" "$stage/entitlements.plist" "$version"

printf '%s' "$MAC_STORE_APP_P12_BASE64" | tr -d '\r\n' | base64 --decode > "$stage/app.p12"
printf '%s' "$MAC_STORE_INSTALLER_P12_BASE64" | tr -d '\r\n' | base64 --decode > "$stage/installer.p12"
printf '%s' "$MAC_STORE_PROFILE_BASE64" | tr -d '\r\n' | base64 --decode > "$app/Contents/embedded.provisionprofile"
unset MAC_STORE_APP_P12_BASE64 MAC_STORE_INSTALLER_P12_BASE64 MAC_STORE_PROFILE_BASE64
security cms -D -i "$app/Contents/embedded.provisionprofile" > "$stage/profile.plist"
python3 - "$stage/profile.plist" "$stage/entitlements.plist" <<'PY'
from datetime import datetime, timezone
import plistlib
import sys

with open(sys.argv[1], "rb") as stream:
    profile = plistlib.load(stream)
with open(sys.argv[2], "rb") as stream:
    requested = plistlib.load(stream)
allowed = profile.get("Entitlements", {})
team = requested["com.apple.developer.team-identifier"]
identifier = requested["com.apple.application-identifier"]
profile_id = allowed.get("com.apple.application-identifier", allowed.get("application-identifier"))
if profile_id != identifier or team not in profile.get("TeamIdentifier", []):
    sys.exit("Provisioning profile does not match the Store bundle ID and team")
if profile["ExpirationDate"].replace(tzinfo=timezone.utc) <= datetime.now(timezone.utc):
    sys.exit("Provisioning profile has expired")
if profile.get("ProvisionedDevices") or profile.get("ProvisionsAllDevices") or allowed.get("get-task-allow") or allowed.get("com.apple.security.get-task-allow"):
    sys.exit("Use a Mac App Store distribution provisioning profile")
PY

security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 3600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$stage/app.p12" -k "$keychain" -P "$MAC_STORE_APP_P12_PASSWORD" -T /usr/bin/codesign
security import "$stage/installer.p12" -k "$keychain" -P "$MAC_STORE_INSTALLER_P12_PASSWORD" -T /usr/bin/productbuild
unset MAC_STORE_APP_P12_PASSWORD MAC_STORE_INSTALLER_P12_PASSWORD
security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_password" "$keychain" >/dev/null

app_identity=$(security find-identity -v -p codesigning "$keychain" |
    awk '/"Apple Distribution:|"3rd Party Mac Developer Application:/ {print $2; exit}')
installer_identity=$(security find-identity -v "$keychain" |
    awk -F'"' '/"3rd Party Mac Developer Installer:|"Mac Installer Distribution:/ {print $2; exit}')
[[ -n $app_identity ]] || { echo "Mac App Distribution identity missing" >&2; exit 1; }
[[ -n $installer_identity ]] || { echo "Mac Installer Distribution identity missing" >&2; exit 1; }

codesign --force --keychain "$keychain" --sign "$app_identity" \
    --entitlements "$stage/entitlements.plist" "$app"
codesign --verify --deep --strict "$app"
codesign -d --extract-certificates "$stage/verified-cert" "$app"
python3 - "$stage/profile.plist" "$stage/verified-cert0" <<'PYTHON'
import plistlib
import sys

with open(sys.argv[1], "rb") as stream:
    profile = plistlib.load(stream)
with open(sys.argv[2], "rb") as stream:
    signing_certificate = stream.read()
if signing_certificate not in profile.get("DeveloperCertificates", []):
    sys.exit("Provisioning profile does not authorize the application signing certificate")
PYTHON
codesign -dv --verbose=4 "$app" 2>&1 | awk -v expected="TeamIdentifier=$MAC_STORE_TEAM_ID" '$0 == expected { matched = 1 } END { exit !matched }'
codesign -d --xml --entitlements - "$app" > "$stage/signed-entitlements.plist"
[[ $(/usr/libexec/PlistBuddy -c "Print :com.apple.security.app-sandbox" "$stage/signed-entitlements.plist") == true ]]

mkdir -p "$output"
pkg="$output/Terraphim-Editor-Mac-App-Store.pkg"
productbuild --keychain "$keychain" --sign "$installer_identity" \
    --component "$app" /Applications "$pkg"
pkgutil --check-signature "$pkg"
(cd "$output" && shasum -a 256 Terraphim-Editor-Mac-App-Store.pkg > SHA256SUMS)
