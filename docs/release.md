# Release & Code Signing

> This document was moved out of the README. It describes how the release pipeline works, how Tauri updater signing is configured, and what the Windows SmartScreen warning means.

## Release pipeline

The project builds installers and publishes them to GitHub Releases automatically via GitHub Actions; the workflow is defined in [`.github/workflows/release.yml`](../.github/workflows/release.yml).

**Triggers:**

| Trigger | Action | Artifact |
| --- | --- | --- |
| Push version tag | `git tag v0.3.0 && git push origin v0.3.0` | Published Release, version from tag |
| Manual | Repo → Actions → Release → Run workflow | Draft Release, version `0.0.0-dev.<run#>` |

The workflow syncs the tag version into `tauri.conf.json` before building, so artifacts never share a duplicate version number.

## Tauri Updater signing keys (optional, for auto-update verification)

The workflow reads two secrets: `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. They produce a `.sig` signature for the installer, which Tauri's built-in updater uses to verify installer integrity during auto-updates — **this is the Tauri updater signature, not Windows code signing**.

Generate a keypair once locally (keep the private key safe; if lost, already-installed old versions cannot receive new updates):

```bash
pnpm tauri signer generate -w ~/.tauri/gitworkspace.key
```

This prints a public key (`dW50cnVzdGVk...`) and writes the private key file. Then configure two secrets under the repo's Settings → Secrets and variables → Actions:

| Secret | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Private key file content (or path) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password set when generating the key (leave empty if none) |

> Once configured, the build also emits a `*.exe.sig` signature file under `target/release/bundle/nsis/`. To enable auto-updates, also fill the public key into `tauri.conf.json`'s `plugins.updater.pubkey`.

## On the Windows SmartScreen "unknown publisher" warning (important clarification)

The Tauri updater signature above does **not** clear the SmartScreen warning. SmartScreen recognizes **Windows Authenticode code-signing certificates**, which must be purchased (OV/EV) from a trusted CA (e.g. DigiCert, Sectigo) — a separate mechanism:

- Configure `certificateThumbprint`, `digestAlgorithm` (e.g. `sha256`) and `timestampUrl` (timestamp server) under `bundle.windows` in `tauri.conf.json`;
- In CI, store the `.pfx` certificate base64-encoded as a GitHub Secret (e.g. `WINDOWS_CERTIFICATE` / `WINDOWS_CERTIFICATE_PASSWORD`), decode and import it before build, then `tauri build` invokes `signtool` automatically.

To clear the SmartScreen warning, obtain an Authenticode certificate and follow the [Tauri Windows code signing guide](https://v2.tauri.app/distribute/sign/windows/).
