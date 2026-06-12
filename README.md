# wlbal

Work-Leisure Balance is a strict macOS desktop Pomodoro-style enforcer built with Tauri v2, React, and TypeScript.

## Requirements

- macOS 13+
- Node.js 18+
- Rust stable
- Xcode Command Line Tools

## Development

```sh
npm install
npm run tauri dev
```

Do not open `src-tauri/target/debug/wlbal-desktop` directly unless the Vite dev server is already running. The debug binary points at `http://127.0.0.1:1420`, so opening it by itself can show a blank window.

If you want to run the raw debug binary manually:

```sh
npm run dev
./src-tauri/target/debug/wlbal-desktop
```

For a double-clickable app, build and open the macOS app bundle:

```sh
npm run tauri build -- --bundles app
open src-tauri/target/release/bundle/macos/wlbal.app
```

## Sharing A DMG

macOS Gatekeeper blocks unsigned or unnotarized apps downloaded from another machine. A DMG produced by `npm run tauri build` is fine for local testing, but another person may see "cannot be opened", "developer cannot be verified", or "is damaged and can't be opened" after dragging it to Applications.

For local/internal testing without an Apple Developer ID, create an ad-hoc signed DMG:

```sh
npm run package:mac:local
```

The output is:

```sh
src-tauri/target/release/bundle/local/wlbal-local-unsigned.dmg
```

If Gatekeeper still blocks it on the recipient's Mac, they can remove quarantine after installing:

```sh
xattr -dr com.apple.quarantine /Applications/wlbal.app
open /Applications/wlbal.app
```

For a DMG that opens normally for other people, build with a Developer ID Application certificate and notarize it with Apple. Ad-hoc signing cannot replace notarization for public sharing.

The app stores config at:

```sh
~/.config/wlbal/config.json
```

The audit log is written to:

```sh
~/.config/wlbal/log.json
```

## CLI

The desktop app listens on:

```sh
/tmp/wlbal.sock
```

Build the companion CLI:

```sh
cd src-tauri
cargo build --bin wlbal
```

From the app onboarding screen, use **Install CLI** to copy the binary to `/usr/local/bin/wlbal` with administrator approval. During development you can also run it directly:

```sh
./src-tauri/target/debug/wlbal status
./src-tauri/target/debug/wlbal switch 10m
./src-tauri/target/debug/wlbal pause
./src-tauri/target/debug/wlbal resume
```

Socket smoke test:

```sh
echo '{"command":"status"}' | nc -U /tmp/wlbal.sock
```

## Permissions

Website blocking has two layers:

- Browser URL enforcement redirects blocked active tabs to a local wlbal block page in Safari, Chrome, Brave, Edge, Arc, Vivaldi, Opera, Chromium, Firefox, Firefox Developer Edition, LibreWolf, Waterfox, Floorp, and Zen Browser.
- Optional `/etc/hosts` blocking can be enabled in Settings with **Use /etc/hosts Blocking**. It is off by default to avoid repeated administrator password prompts.

macOS may ask for Automation permission the first time wlbal controls a browser. Firefox-family browsers also require Accessibility/UI scripting permission because they do not expose active tab URLs through AppleScript; wlbal checks them only when they are frontmost. Allow these prompts, otherwise browser URL enforcement cannot redirect tabs. This layer is useful when a VPN, secure DNS, or corporate proxy makes `/etc/hosts` unreliable.

App blocking enumerates installed apps from `/Applications` and `~/Applications`, maps bundle IDs to executables, polls every two seconds, and sends `SIGTERM` to blocked processes during the active phase.

## Behavior

- GUI has no phase switch, pause, or skip control.
- **Get to Work** runs the user-configured shell script from Settings using the user's default shell.
- Enforcement is disarmed on every app launch until the user clicks **Arm** or finishes onboarding with **Start wlbal**.
- Clicking **Arm** first saves the current UI config, then applies website rules for the active phase.
- Clicking **Disarm** suspends app blocking and removes wlbal's active hosts block if hosts blocking is enabled.
- Disarming resets the timer to a fresh Work session.
- Saving schedule changes restarts the current phase with the new duration.
- Natural phase progression alternates Work and Leisure.
- `wlbal switch <duration>` switches to the opposite phase temporarily.
- `wlbal pause` suspends both the timer and enforcement; `wlbal resume` resumes the timer, and enforcement resumes only if the app is armed.
- Override duration is capped at 2 hours.
- `continue` override recovery resumes the interrupted phase with its previous remaining time.
- `fresh_cycle` override recovery starts a fresh Work session.

## Get-To-Work Script

Configure this in Settings. The script is saved in `~/.config/wlbal/config.json` and runs only when you click **Get to Work**.

Example:

```sh
open -a "Visual Studio Code" ~/work
open https://linear.app
```

The script runs through `$SHELL -lc`, so do not paste scripts you do not trust.
