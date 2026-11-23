#!/usr/bin/env bash
set -euo pipefail

binary=$1
app=$2
entitlements=$3
version=$4

: "${MAC_STORE_BUNDLE_ID:?Set the registered Mac App Store bundle ID}"
: "${MAC_STORE_TEAM_ID:?Set the Apple team ID}"
: "${MAC_STORE_BUILD_NUMBER:?Set the App Store Connect build number}"
[[ $MAC_STORE_BUNDLE_ID =~ ^[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+$ ]]
[[ $MAC_STORE_TEAM_ID =~ ^[A-Z0-9]{10}$ ]]
[[ $MAC_STORE_BUILD_NUMBER =~ ^[1-9][0-9]*$ && ${#MAC_STORE_BUILD_NUMBER} -le 18 ]]
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
[[ $app == *.app && ! -e $app && ! -e $entitlements ]]
lipo -verify_arch arm64 "$binary"

mkdir -p "$app/Contents/MacOS"
install -m 755 "$binary" "$app/Contents/MacOS/terraphim-gpui-editor"

python3 - "$app/Contents/Info.plist" "$entitlements" "$version" <<'PY'
import os
import plistlib
import sys

info, entitlements, version = sys.argv[1:]
team = os.environ["MAC_STORE_TEAM_ID"]
identifier = os.environ["MAC_STORE_BUNDLE_ID"]
with open(info, "wb") as stream:
    plistlib.dump({
        "CFBundleExecutable": "terraphim-gpui-editor",
        "CFBundleIdentifier": identifier,
        "CFBundleName": "Terraphim Editor",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": os.environ["MAC_STORE_BUILD_NUMBER"],
        "LSApplicationCategoryType": "public.app-category.productivity",
        "LSMinimumSystemVersion": "14.0",
        "NSHighResolutionCapable": True,
    }, stream)
with open(entitlements, "wb") as stream:
    plistlib.dump({
        "com.apple.application-identifier": f"{team}.{identifier}",
        "com.apple.developer.team-identifier": team,
        "com.apple.security.app-sandbox": True,
        "com.apple.security.files.user-selected.read-write": True,
    }, stream)
PY
plutil -lint "$app/Contents/Info.plist" "$entitlements"
