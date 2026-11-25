# Releasing Hopr

## Overview

- **Source repository**: `nowshadabir/hopr` (private)
- **Public distribution repository**: `nowshadabir/hopr-releases` (public GitHub Releases for DMGs + GitHub Pages `gh-pages` branch for `appcast.xml`)
- **Permanent Bundle ID**: `com.hopr.app` (do not change after `v1.0.0`)
- **Permanent Appcast URL**: `https://nowshadabir.github.io/hopr-releases/appcast.xml` (do not change after `v1.0.0`)
- **Minimum macOS**: `14.0`

## How to Cut a Release

1. Update `CHANGELOG.md` and `Cargo.toml` version (e.g. `1.1.0`).
2. Commit and push to `main`.
3. Create an annotated tag and push it:
   ```zsh
   git tag -a v1.1.0 -m "Hopr 1.1.0"
   git push origin v1.1.0
   ```
4. GitHub Actions (`.github/workflows/release.yml`) will:
   - Run `cargo test`
   - Build a universal binary (`aarch64-apple-darwin` + `x86_64-apple-darwin`)
   - Embed `Sparkle.framework` (`2.10.1`, SHA-256 verified) and sign inside-out
   - Package `Hopr-<version>.dmg`
   - Notarize & staple if Apple Developer credentials are configured
   - Sign the DMG with `SPARKLE_ED_PRIVATE_KEY`
   - Publish the DMG to `nowshadabir/hopr-releases` and update `appcast.xml` on `gh-pages`

## How to Roll Back a Bad Release

1. Clone `nowshadabir/hopr-releases` (`gh-pages` branch).
2. Remove the bad `<item>` from `appcast.xml` and push `gh-pages`.
3. Fix the issue on `main` and ship a **higher** version tag (e.g. `v1.1.1`) so `CFBundleVersion` strictly increases.

## Required Secrets (in `nowshadabir/hopr` Settings → Secrets and variables → Actions)

- `SPARKLE_ED_PRIVATE_KEY`: Sparkle EdDSA private key (**back this up in a password manager; if lost, installed apps cannot verify future updates**).
- `RELEASES_REPO_TOKEN`: GitHub PAT with `repo` scope so CI in the private `nowshadabir/hopr` repo can publish releases and push `gh-pages` to `nowshadabir/hopr-releases`.
- Optional (for Apple Developer ID signing & notarization when available):
  - `DEVELOPER_ID_CERT_P12_BASE64`
  - `DEVELOPER_ID_CERT_PASSWORD`
  - `APPLE_TEAM_ID`
  - `APPLE_API_KEY_ID`
  - `APPLE_API_ISSUER_ID`
  - `APPLE_API_KEY_P8`

## Rotating Secrets

- **Sparkle key**: Never rotate `SPARKLE_ED_PRIVATE_KEY` without shipping a transition release that includes the new `SUPublicEDKey` signed by the old private key first.
- **GitHub PAT / Apple keys**: Update the secret in GitHub Actions settings; no app rebuild is required.
