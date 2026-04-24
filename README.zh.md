# TeRmalM

[English](./README.md)

一个桌面端终端任务管理工具 — 在本地或远程服务器上运行命令，让任务在后台持续执行，随时查看状态和日志。

## 功能

- **本地与 SSH 任务** — 在本机或通过 SSH 在远程服务器上执行命令
- **后台持续运行** — 关闭终端标签后任务仍在运行，随时回来查看状态和日志
- **交互式终端** — 为任意任务打开实时终端标签，本地和远程均支持
- **SSH 配置集成** — 直接读取 `~/.ssh/config` 中的主机别名，不存储任何凭证
- **开机自启动** — 将任务注册为系统启动项，开机自动运行

## 下载

前往 [Releases](../../releases) 页面，根据你的平台选择对应文件：

| 平台 | 文件 |
|---|---|
| Windows | `TeRmalM-windows-x64-setup.exe` 或 `TeRmalM-windows-x64.msi` |
| macOS Apple Silicon（M1/M2/M3/M4） | `TeRmalM-mac-apple-silicon.dmg` |
| macOS Intel | `TeRmalM-mac-intel.dmg` |
| Linux | `TeRmalM-linux-x64.deb` / `.rpm` / `.AppImage` |

### macOS 提示

由于应用未经 Apple 公证，macOS 可能会提示"已损坏"。安装后在终端执行以下命令即可正常打开：

```sh
xattr -cr /Applications/TeRmalM.app
```

## 开发

依赖：[Node.js](https://nodejs.org) 和 [Rust 工具链](https://rustup.rs)。

```sh
npm install

# 浏览器预览（使用 mock API）
npm run dev

# Tauri 桌面应用
npm run tauri:dev

# 构建安装包
npm run tauri:build
```

> 进程执行、PTY 会话、SQLite 存储、SSH 配置解析和开机自启动功能仅在 Tauri 桌面应用中可用，浏览器预览使用 mock API。

## 许可证

GNU General Public License v3.0 — 详见 [LICENSE](./LICENSE)。
