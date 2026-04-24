# TeRmalM

[中文](./README.zh.md)

A desktop app for managing terminal command tasks — run them locally or over SSH, keep them running in the background, and check in whenever you want.

## Features

- **Local & SSH tasks** — run commands on your machine or on a remote server via SSH
- **Background execution** — tasks keep running after you close the terminal tab; come back to check status and logs anytime
- **Interactive terminal** — open a live terminal tab for any task, local or remote
- **SSH config aware** — reads host aliases directly from `~/.ssh/config`; no credential storage
- **Autostart** — register tasks to launch automatically on system boot

## Download

Go to [Releases](../../releases) and pick the file for your platform:

| Platform | File |
|---|---|
| Windows | `TeRmalM-windows-x64-setup.exe` or `TeRmalM-windows-x64.msi` |
| macOS Apple Silicon (M1/M2/M3/M4) | `TeRmalM-mac-apple-silicon.dmg` |
| macOS Intel | `TeRmalM-mac-intel.dmg` |
| Linux | `TeRmalM-linux-x64.deb` / `.rpm` / `.AppImage` |

## Development

Prerequisites: [Node.js](https://nodejs.org) and the [Rust toolchain](https://rustup.rs).

```sh
npm install

# Browser preview (mock APIs)
npm run dev

# Tauri desktop app
npm run tauri:dev

# Build installers
npm run tauri:build
```

> Process execution, PTY sessions, SQLite storage, SSH config parsing, and autostart only work inside the Tauri desktop app. The browser preview uses mock APIs.

## License

GNU General Public License v3.0 — see [LICENSE](./LICENSE).
