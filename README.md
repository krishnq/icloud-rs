# iCloud-RS (CloudSync)

A high-performance, asynchronous Rust client for Apple iCloud services, featuring a Tauri-based React frontend and native FUSE virtual filesystem integrations.

## Features

- **Tauri + React GUI**: A lightweight, modern user interface to manage your iCloud sessions and sync settings.
- **Native FUSE Integration**: Mount your iCloud Drive and iCloud Photos directly into your Linux filesystem! Browse your cloud files natively with zero upfront downloading using standard tools like `ls`, `cat`, or desktop file managers (Nautilus, Dolphin).
- **Secure Keyring Storage**: Session cookies and authentication tokens are securely encrypted and stored in your OS's native keyring (Secret Service API / KWallet / Keychain).
- **Zero-Amplification Streaming**: Optimized FUSE streaming reads exactly the bytes requested by your applications, preventing massive bandwidth spikes and connection deadlocks when generating thumbnails.
- **Multi-Account Configuration**: Supports persistent configuration in `~/.config/icloud-rs/config.toml` to automatically manage and mount multiple iCloud accounts on system boot.

## How it Works

1. **Authentication**: The Tauri app spawns a hidden Webview pointing to `icloud.com`. Once you sign in natively, the app harvests the authentication cookies and persists them securely into the system Keyring.
2. **Mounting**: The Rust backend intercepts `CPLMaster` records (Photos) and CloudDocs metadata (Drive) to build a virtual, in-memory directory tree.
3. **FUSE Backend**: The `fuser` crate intercepts file system calls. When a file is read, the daemon fetches the exact byte chunks required directly from Apple's CDNs.

## Requirements

- **Rust / Cargo** (1.70+)
- **Node.js** (v18+)
- **FUSE3** (`libfuse3-dev` / `fuse3` installed on Linux)
- A Desktop Environment with Keyring support (KWallet, GNOME Keyring)

## Quick Start

1. Install dependencies:
   ```bash
   cd cloudsync
   npm install
   ```

2. Run the development server:
   ```bash
   npm run tauri dev
   ```

3. Sign in to your iCloud account when prompted. The application will automatically securely cache your session and mount the FUSE drives.

## Project Structure

- `core/`: The asynchronous Rust backend responsible for iCloud API interactions, HTTP streaming, and the FUSE daemon implementations.
- `cloudsync/`: The Tauri wrapper and React frontend application.

## Configuration

The application will auto-generate a configuration file upon first launch at `~/.config/icloud-rs/config.toml`. 

```toml
[accounts.default_icloud]
type = "icloud"
mount_drive = "/data/icloud/drive"
mount_photos = "/data/icloud/photos"
```
You can edit the `mount_drive` and `mount_photos` variables to change where the FUSE daemon creates the virtual drives.
