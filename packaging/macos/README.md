# Mac App Store signing

Run **Sign Mac App Store package** from the default branch with a new positive
App Store Connect build number. Download the signed Apple Silicon package and
its SHA-256 checksum from the workflow artifacts.

Configure the `mac-app-store-signing` GitHub environment:

| Kind | Name | Value |
| --- | --- | --- |
| Variable | `MAC_STORE_BUNDLE_ID` | Explicit bundle ID registered in Apple Developer |
| Variable | `MAC_STORE_TEAM_ID` | Apple Developer Team ID |
| Secret | `MAC_STORE_APP_P12_BASE64` | Base64 PKCS#12 with the Apple Distribution or Mac App Distribution identity and private key |
| Secret | `MAC_STORE_APP_P12_PASSWORD` | Password for the application PKCS#12 |
| Secret | `MAC_STORE_INSTALLER_P12_BASE64` | Base64 PKCS#12 with the Mac Installer Distribution identity and private key |
| Secret | `MAC_STORE_INSTALLER_P12_PASSWORD` | Password for the installer PKCS#12 |
| Secret | `MAC_STORE_PROFILE_BASE64` | Base64 Mac App Store distribution provisioning profile for this app and team |

The Developer ID credentials used by terraphim-clients are for notarized
downloads. Use the Store distribution identities listed above. The packaging
script expects the App ID prefix to equal the Team ID.

Before submission, exercise document and Markdown KG directory access through
native file pickers and persistent sandbox permissions. Resolve the external
`gtr` CLI dependency for Store builds and prepare the icon and App Store Connect
listing. This workflow produces a package for validation. Submission is manual.
