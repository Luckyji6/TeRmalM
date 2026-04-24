# TeRmalM

TeRmalM is a cross-platform desktop app for managing local and SSH-backed terminal commands.

## Stack

- React + TypeScript + Vite for the UI
- Tauri 2 for the desktop shell
- Rust commands for process, PTY, SSH config, and local autostart integration
- SQLite for local task storage
- GitHub Actions for Win/macOS/Linux bundles

## Current MVP

- Create local or SSH command tasks.
- Persist tasks in SQLite under the Tauri app data directory.
- Read SSH host aliases from `~/.ssh/config`.
- Start, stop, poll status, and read logs for background tasks.
- Open xterm-powered local or SSH interactive terminal tabs.
- Generate local system autostart entries for command tasks:
  - macOS: `~/Library/LaunchAgents`
  - Linux: `~/.config/systemd/user`
  - Windows: `schtasks`

Remote system-level autostart is intentionally out of scope for the first MVP. SSH credentials are not stored by TeRmalM; authentication stays in the user's OpenSSH config, agent, and keychain setup.

## Development

Install Node.js and the Rust toolchain first. On macOS/Linux, the usual Rust install path is:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Install dependencies:

```sh
npm install
```

Run the browser preview:

```sh
npm run dev
```

Run the Tauri desktop app:

```sh
npm run tauri:dev
```

Build the frontend:

```sh
npm run build
```

Build desktop bundles:

```sh
npm run tauri:build
```

## Notes

The browser preview uses mock process and terminal APIs. Real process execution, PTY sessions, SQLite storage, SSH config parsing, and autostart generation run only inside Tauri.

## License

TeRmalM is licensed under the GNU General Public License v3.0 only. See [LICENSE](./LICENSE).
