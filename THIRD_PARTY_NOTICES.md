# Third-Party Notices

LocalBridge first-party source is distributed under the MIT License in `LICENSE`. Third-party software remains under its own terms.

This inventory is verified during release preparation and finalized against the exact packaged payload/SBOM before each stable release.

Initial runtime dependencies include:

- coding-tools-mcp — Apache-2.0
- openai/tunnel-client — Apache-2.0
- Python Embedded Runtime — Python Software Foundation License
- aria2 1.37.0 — GPL-2.0-or-later — source `https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-win-64bit-build1.zip` — archive SHA256 `67d015301eef0b612191212d564c5bb0a14b5b9c4796b76454276a4d28d9b288` — packaged executable SHA256 `be2099c214f63a3cb4954b09a0becd6e2e34660b886d4c898d260febfe9d70c2`
- 7-Zip 26.02 standalone console (`7za.exe`, exposed as logical `7z`) — LGPL-2.1-or-later with upstream unRAR restriction notice — source `https://www.7-zip.org/a/7z2602-extra.7z` — archive SHA256 `081df9e9311dfd9c9e0e98c1c80180b99bb51e4cb24156b5f3057fe3c259d70a` — packaged executable SHA256 `35d4d69d7cd6cb44558f208c3b1334268013f9daf82d2dda848893a1c30c59c2`
- jq 1.8.2 — MIT — source `https://github.com/jqlang/jq/releases/download/jq-1.8.2/jq-windows-amd64.exe` — archive/executable SHA256 `a6fc67fedaf9128a3309a1e2ebb8b986aeccf70122ee46d2cb4849e423f0c627`

Application/source dependencies:

- Tauri 2 and Rust crates — licenses are declared by the locked Cargo dependency metadata and are verified by the release preflight.
- React/Vite/TypeScript and npm dependencies — licenses are declared by `package-lock.json` package metadata and are verified by the release preflight.
- Microsoft WebView2 Runtime — supplied by Windows/system installation and **not redistributed** in the LocalBridge bundle.

Windows `curl.exe` is a system runtime dependency and is not redistributed by LocalBridge; LocalBridge resolves only `%SystemRoot%/System32/curl.exe` after its runtime capability probe.

Exact versions, source commits, binary checksums, transitive dependencies and notices are frozen by the release runtime manifest and SBOM. Where an upstream payload includes its own LICENSE/NOTICE material, that material remains with the redistributed payload.
