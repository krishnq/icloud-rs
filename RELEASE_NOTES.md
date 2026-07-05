# Release Notes - v0.1.0 (Initial Release)

Welcome to the initial release of **iCloud-RS** (CloudSync)!

## Core Features
- **iCloud Drive & Photos VFS**: We have successfully implemented a fully functioning FUSE (Filesystem in Userspace) bridge. Your iCloud files can now be mounted locally as real directories and files without needing to download your entire 50GB+ library upfront.
- **Tauri Application Interface**: A lightweight React + Tauri application that seamlessly handles the complex Apple WebAuth flows, securely storing session tokens so you only have to log in once.

## Security Improvements
- **OS Native Keyring Support**: Raw session cookies are now securely encrypted and pushed directly to your Linux Secret Service or macOS Keychain. No more plaintext JSON credential files.

## Performance & Stability Enhancements
- **Dynamic Timestamps**: FUSE nodes now dynamically report correct `SystemTime` metadata, fixing a critical bug where KDE and KIO background workers would infinitely loop or crash when seeing files stamped with the 1970 UNIX Epoch.
- **Multi-Chunk Non-Contiguous Caching**: Replaced the fragile single-block cache with a persistent, offset-mapped chunk cache. Applications like Gwenview can now jump around to read headers and footers of images without repeatedly destroying and re-downloading the same data.
- **Zero-Amplification Streaming**: Drastically reduced the FUSE `read` function's footprint. The VFS now requests exactly the number of bytes specified by the kernel, eliminating aggressive "Read-Ahead Amplification" that was saturating user networks and causing 15-second deadlocks when rendering image thumbnails.
- **Strict FUSE Network Timeouts**: Addressed FUSE's uninterruptible `D-state` kernel hangs by forcing global 30-second `reqwest` timeouts across all background threads.
- **Multi-Account Configuration**: Introduced `~/.config/icloud-rs/config.toml` scaffolding, making it trivially easy to edit mount points and preparing the application to manage multiple iCloud or Google accounts simultaneously.
