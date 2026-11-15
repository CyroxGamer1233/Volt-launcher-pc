# Volt Launcher — repo map and debug APK

Working notes for the `arena/8fa03d72-volt-launcher` session (base `53883055b`, 2026-11-15).
Local scratch document; delete it if it should not stay in the tree.

## What this repository is

`entitybrian69-bit/Volt-launcher` is the **Volt Launcher** distribution of
**Horizon Android** — an unofficial Android fork of
[Horizon](https://github.com/ludovic111/horizon), a from-scratch Rust + wgpu
reimplementation of **Minecraft: Java Edition 26.3**. Players install an APK, the
app downloads Mojang's original resources itself, and the engine runs natively on
the phone (no Java runtime, no JVM, no Minecraft jar at play time).

- Vanilla Java 26.3 is the specification; the PC/desktop launcher is
  GPUI/Rust (`launcher/`), the game is `crates/horizon-client` (+ 32 more crates).
- Branding on top of upstream: `ae.net.Voltlauncher` (stable) and
  `ae.net.Voltlauncher.debug` ("Volt Debug"), Volt theme (Liquid Glass /
  Liquid Black), dashboard, Controls Studio, mod catalog, updater.

### Layout

| Path | What it holds |
| --- | --- |
| `crates/horizon-client` | The game: `src/lib.rs` (flat modules) + `src/main.rs` → `game.rs`, `game/`; also `bin/horizon-ctl` and the dedicated server |
| `crates/horizon-client/src/android/` | **Volt Android launcher UI**: `launcher.rs`, `launcher_pages.rs`, `catalog.rs`, `controls_studio.rs`, `volt_updater.rs`, `volt_accounts.rs`, `login.rs`, `secure_store.rs`, `diagnostics.rs`, `native_ime.rs`, … |
| `crates/horizon-client/src/game/` | Gameplay per feature: `android_input.rs`, `pad.rs`, `touch_ui`, `remote*.rs`, `lan*.rs`, `integrated.rs`, `dedicated.rs`, mods, screens |
| `crates/horizon-client-render` | World data, rendering, HUD/menus/settings (~80 modules re-exported as `horizon_client::*`) |
| `crates/horizon-world` | Vanilla world data: NBT, regions, chunks, `datafix/`, `worldgen/`, `level/` |
| `crates/horizon-core`, `horizon-protocol`, `horizon-assets`, `horizon-command`, `horizon-items`, `horizon-mod*`, `horizon-theme`, … | 33 crates total (see `AGENTS.md` table) |
| `crates/horizon-android` | The `cdylib` `libhorizon_android.so` wrapper (NativeActivity + Rust IME bridge) |
| `crates/horizon-client/src/touch*`, `ui_theme.rs` | Touch schemes (joystick/D-pad × crosshair/tap), HUD editor, theme |
| `packaging/android/` | `AndroidManifest.xml`, `build.gradle` (namespaces/app ids/versions), `java/` (2 stock-Java classes), `res/`, `aidl/` |
| `tools/android_*.py` | `android_build.py` (Gradle + NDK + signing + audit), `android_prepare.py` (engine tables from your own MC install), `android_release.py`, `android_ctl.py`, probes |
| `tests/test_android*.py` | Release audit, source export, release schedule, public-tree checks |
| `.github/workflows/` | `android-apk.yml` (the APK pipeline), `android-source.yml`, `native-mod-probe.yml`, `pages.yml` |
| `launcher/` | Desktop GPUI launcher (separate workspace), not shipped on Android |
| `docs/` | `android.md` (build/run), `android-publication.md`, `android-features.md`, `RELEASE-NOTES-*.md`, upstream reviews |

### How the APK is produced (`android-apk.yml`)

1. **plan** — ABI matrix (default: all four), profile (`release`/`fast`), publish flag.
2. **prepare** — `tools/android_prepare.py` turns the *player's own* Minecraft 26.3
   install into allowlisted engine tables (never committed, never Mojang files).
3. **weave** — 37 registry `.hnmod` native mods + 1 vendored (`punchy`) are woven
   into the engine library with pinned sha256s.
4. **build / APK per ABI** — unit tests, Gradle 8.14, `tools/android_build.py --public`,
   deterministic signing keystore (`ANDROID_KEYSTORE_BASE64` or derived), payload audit.
5. **release** — `push` to `android`/`arena/**` or dispatch refreshes the single rolling
   **`debug`** release; `v*` tags publish the stable channel. Artifacts are also
   attached to the run (30-day retention).

APK details: `--public` release profile = opt level 3 + ThinLTO, **non-debuggable**,
paths remapped, agent-control socket off. `minSdk 28` (Android 9), `targetSdk 35`,
`versionName 1.0.0`, versionCode = `280000 + run_number*2 + abi_offset`.
The **debug channel** is a *separate app* (`ae.net.Voltlauncher.debug`, "Volt Debug")
so it never overwrites a stable install.

## Branches

`android` is the long-lived branch; everything else is `arena/*` session branches
(one per Arena session), plus a junk `check-log-branch`.

| Branch | Tip | State |
| --- | --- | --- |
| `android` | `53883055b` (2026-11-15) | **Current base.** Merge of PR #3 (`arena/e1f86b34`): "Ship Volt Android launcher updates and login fixes" — modem/authentication fixes, login packet ordering, launcher alignment, live Android mod reload, key capture, shader metadata. |
| `arena/0cae2b73-volt-launcher` | `8594f2b7c` (2026-11-15) | **PR #4, open**, `+83,883 / −4,348` over 328 files, 259 commits ahead / 72 behind `android`. Newest work: debug-channel app split, sneak reliability, joystick toggle, "Frame" items, Liquid Glass silver redesign, Liquid Black "Embers" theme, dashboard + embedded Controls Studio, 3D skins, server-join vanilla fingerprint fixes (captcha-failed kicks). **Source of the current debug APK.** |
| `arena/e1f86b34-volt-launcher` | `c443b8a30` | merged (PR #3) → `android` |
| `arena/42c008a1-volt-launcher` | `e11d9c788` (2026-11-07) | merged (PR #1): Volt branding + screenshot gallery (docs only) |
| `arena/c3d6069d-volt-launcher` | `6b225e4d6` (2026-11-13) | **PR #2, open, stale** — 4 commits, none in `android` or the PR #4 tip: dark-glass monochrome overhaul, app branding, APK rename, then it pulls in the full PR #4 branch. Likely to be superseded by PR #4 (which already carries Liquid Glass/Liquid Black). |
| `arena/58a3cdf9-volt-launcher` | `3c6a6ad50` (2026-10-31) | no PR; "Brand Android app as Volt Launcher" — early predecessor, 1,700+ lines of docs behind |
| `arena/a4df1109-volt-launcher` | `9199f02ad` (2026-11-14) | no PR; spike that reverted an extra `horizon-client-render` dep that broke the weave |
| `check-log-branch` | `5f7b9689b` | throwaway CI log dump (`check.log`), tag `check-log` with a `check.log` asset |

Repo tags/releases: `v1.0.0` (stable, `e413ea1a4`), `v0.7.6-build.939d02a` (pre-release),
rolling `debug`, `check-log`.

## Debug APK — where it is and why it is not attached here

Current debug build: **Volt Launcher Debug Build (1.0.0 @ 8594f2b)**, published
2026-11-15 10:24 UTC from `8594f2b7c` (= PR #4 head), successful CI run
[43701289653](https://github.com/entitybrian69-bit/Volt-launcher/actions/runs/43701289653)
(all 12 jobs green: plan, prepare, worldgen tests, runtime proof, 4× weave, 4× build, release).

Release page: <https://github.com/entitybrian69-bit/Volt-launcher/releases/tag/debug>

| Asset | ABI | Size | sha256 |
| --- | --- | --- | --- |
| `Volt-Launcher-debug-arm64-v8a.apk` | arm64-v8a (phones) | 73,188,131 B | `50cade8f09caa8e01a7160d0dccf71f1cf6e28b507e16fed44c19613f97bd3fb` |
| `Volt-Launcher-debug-armeabi-v7a.apk` | armeabi-v7a (32-bit) | 51,299,109 B | `3af80746021f40aa57449c1970a73a9a7b7621f4b519a92c0afe155979fcd1be` |
| `Volt-Launcher-debug-x86_64.apk` | x86_64 (emulator/ChromeOS) | 80,757,536 B | `511c3c804938f5e062b5dd491042ed32ee0b921e1fcdf72008e19b12c0996813` |
| `Volt-Launcher-debug-x86.apk` | x86 | 73,433,885 B | `32d9d73ce1e9c16fd04e85bc276426cf675dca33f82c8fc14f5bd316040952e2` |

Also attached: `biomes_unseen.hmod` (44 biomes, 176 blocks, 29 items), `SHA256SUMS.txt`,
`LICENSE.txt`, `THIRD_PARTY_NOTICES.md`.

Per-run CI artifacts for `android` @ `53883055b` (37 MB arm64-v8a zip, run 43700660342)
expire after 30 days.

**Why no APK file sits in this workspace:** this sandbox's outbound policy allows only
`github.com`, `codeload.github.com`, `api.github.com`, `registry.npmjs.org`, `pypi.org`
and `files.pythonhosted.org`. GitHub serves release assets and Actions artifacts from
`release-assets.githubusercontent.com`, `objects.githubusercontent.com` and
`*.blob.core.windows.net` (302 redirects), so every download attempt fails with
`SSL_ERROR_SYSCALL`. A local build is equally impossible: no Rust toolchain, JDK,
Android SDK/NDK or Gradle, and `static.rust-lang.org`, `dl.google.com`,
`services.gradle.org`, `repo.maven.apache.org` and `piston-meta.mojang.com` are all
unreachable, plus a 21 GB / 2-core budget for a ~1 hour multi-ABI Rust build.
The APK must be downloaded from the release page above in a normal browser, or built
with `python tools/android_build.py --abi arm64-v8a --public` (needs SDK 35, NDK
28.2.13676358, JDK 17, Gradle 8.14, Rust 1.95 + `aarch64-linux-android`, and a one-time
`tools/android_prepare.py` against a licensed Minecraft 26.3 install).

### Install notes

- Enable "install unknown apps" for your browser; the debug APK is signed with the
  project's persistent debug certificate, so it updates in place only over earlier
  Volt **Debug** installs — it installs beside the stable `ae.net.Voltlauncher`.
- First launch: pick **Download Minecraft 26.3** (~500 MiB, official Mojang HTTPS,
  hash-pinned, resumable), then **Play**. Vulkan is the default renderer with a GLES
  fallback; Android 9+ (API 28); arm64 build ships the native `.hmod` runtime.
- Verify integrity with `sha256sum Volt-Launcher-debug-arm64-v8a.apk`.

## How the pieces fit (mental model)

- **Two launchers, one engine.** `launcher/` is the desktop GPUI launcher; on Android
  the launcher is *in-engine* UI from `crates/horizon-client/src/android/`. The APK
  contains one `libhorizon_android.so` and two small stock Java classes; no duplicated engine.
- **Launcher → installer → game.** The launcher downloads/verifies resources, then
  re-execs the same binary into the game loop (`game.rs`). Leaving the game returns to
  the launcher.
- **One simulation for LAN/SP/multiplayer.** Single player runs on an in-process server
  thread (`game/integrated.rs`); the window joins it like a LAN guest, so fixes serve
  all three paths. `--in-frame` (old path) is deprecated.
- **Rules that bite:** never commit Mojang files or prepared resources; `--public` for
  anything distributed; `--release-candidate`/`--public` audits are privacy checks, not
  redistribution permission; keep signing keys in `.local/android/`; upstream deployment
  workflows stay inactive on this fork.
