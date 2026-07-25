English | [简体中文](./README.zh-CN.md)

🌐 Website: https://cdl.8421.fun

# CDiskLinker

A Windows tool that frees up C drive space by moving large folders to other drives. After migration, your apps still work from their original paths — no reinstall or reconfiguration needed.

## Features

- **Safe Migration**: Copy → per-file verification → rename source → create junction → user confirms → delete old source. Every step is guarded by SHA256 checksums to guarantee zero data loss
- **Apps Keep Working**: Automatically creates a Directory Junction at the original location, so apps access files from the original path with no awareness of the move
- **Crash Recovery**: After a power failure or crash, reopening the tool resumes from the exact checkpoint
- **Instant Rollback**: If an app malfunctions after migration, one-click rollback restores the original state in seconds — no data copying needed
- **Lock Detection**: Scans for file locks before migration. If deletion fails, it pinpoints exactly which file is held by which process
- **Async Scanning**: Directory tree opens instantly; file sizes are computed in the background
- **Auto Elevation**: Automatically detects and requests administrator privileges

## Important Notes

### Which Directories Can Be Safely Migrated

✅ **Usually safe to migrate**:
- Game platform directories (e.g. Steam, Epic Games)
- Chat app caches (e.g. WeChat, QQ file cache directories)
- Developer tool directories (e.g. Node.js global packages, Maven repository)
- Download and temporary file directories

⚠️ **May have issues — confirm before migrating**:
- **Some apps detect real paths**: A few apps distinguish between shortcuts and real folders, which may cause them to malfunction after migration
- **Never migrate system directories**: `Windows`, `Program Files`, `Users` and other critical system directories must never be migrated — doing so may make the system unbootable
- **Encrypted/DRM-protected software**: Some encrypted software may be bound to physical paths and require reactivation after migration
- **Apps with hardcoded registry paths**: If an app records its install path in the registry, you may need to manually edit the registry or reinstall after migration

### Must-Read Before Migration

1. **Verify app compatibility**: The easiest way is to ask an AI (e.g. "Can [app name]'s install directory be migrated to another drive using a symbolic link / directory junction?"). You can also test it practically:
   - Create a subfolder inside the app's install directory and put a few files in it
   - Migrate this subfolder to another drive with this tool
   - Open the app and confirm everything works before migrating the entire directory
   - If migration fails, the tool rolls back automatically; if migration succeeds but the app errors, click "Rollback" in the confirmation dialog for instant restoration
2. **Ensure sufficient target drive space**: The target drive must be larger than the source directory (with at least 1GB headroom)
3. **Close running apps**: Close apps that are using files in the source directory before migration to avoid file lock failures
4. **Target drive must be NTFS**: FAT32/exFAT and other file systems are not supported

## Safety Guarantees

1. **Per-file Verification**: Before any irreversible operation, every file's path, size, and SHA256 must match exactly — otherwise the source is preserved
2. **Rename-First**: The source directory is renamed (not deleted) before creating the junction, so the original data is always intact until you confirm
3. **User Confirmation**: After migration, you must test the app and confirm it works before the old source is deleted
4. **Instant Rollback**: If the app doesn't work after migration, click "Rollback" to restore the original state in seconds — no data copy needed
5. **Lock Detection**: Scans for file locks before migration; pinpoints the locked file and process on deletion failure
6. **Junction Safety**: Preserves directory junction structure during copy; deletes only the junction, never the target data
7. **Single-Copy Protection**: After the old source is deleted, the target data is the only copy — no step failure will delete it
8. **Auto Repair**: When source files are modified during migration, the tool automatically syncs the differences and re-verifies

## Development

```bash
# Install dependencies
npm install

# Development mode
npx tauri dev

# Build
npx tauri build
```

## Project Structure

```
CDiskLinker/
├── src/                    # Vue 3 frontend
│   ├── components/         # UI components
│   ├── stores/             # Pinia state management
│   └── views/              # Page views
├── src-tauri/              # Rust backend
│   └── src/
│       ├── engine.rs       # Migration engine (verify, copy, delete, link)
│       ├── win_util.rs     # Windows API (junction, file lock, elevation)
│       ├── journal.rs      # Transaction journal (crash recovery)
│       ├── scanner.rs      # Directory scanner
│       └── commands.rs     # Tauri command bridge
└── docs/knowledge-base/    # Knowledge base (constraints, flows, glossary)
```

## License

MIT
