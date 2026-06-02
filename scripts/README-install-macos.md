# photohelper — macOS Installation

## Requirements

- macOS 13 (Ventura) or later
- Apple Silicon (arm64) or Intel (x86_64) — use the correct archive for your Mac

## Install

```bash
# 1. Untar
tar xzf photohelper-VERSION-ARCH.tar.gz
cd photohelper-VERSION-ARCH

# 2. Allow macOS to run the binary (v0.1 is not yet notarized)
xattr -dr com.apple.quarantine photohelper photohelper.sh libonnxruntime.dylib

# 3. Copy the whole directory to a permanent location
mkdir -p ~/Applications/photohelper
cp -R . ~/Applications/photohelper/

# 4. Symlink the wrapper script to your PATH
sudo ln -sf ~/Applications/photohelper/photohelper.sh /usr/local/bin/photohelper

# OR: just run directly from the archive directory
./photohelper.sh --help
```

## Why the wrapper script?

The archive contains `photohelper.sh` which sets `DYLD_LIBRARY_PATH` so macOS can
find `libonnxruntime.dylib` (the AI runtime) next to the binary. Use `photohelper.sh`
instead of `photohelper` directly.

## Verify

```bash
photohelper --help   # via the symlink
# or:
./photohelper.sh --help
```

## Uninstall

```bash
sudo rm /usr/local/bin/photohelper
sudo rm /usr/local/lib/libonnxruntime.dylib
rm -rf ~/photohelper
```

## Notes

- The `PHOTOHELPER_MODEL_DIR` environment variable must point to the `models/`
  directory at runtime. The models are required for `cull` and `dedup`
  subcommands. They are not needed for `ingest`, `develop`, `export`,
  `watermark`, or `rename`.

- v0.1 binaries are not code-signed. macOS will show a Gatekeeper warning on
  first run. The `xattr -dr com.apple.quarantine` step in the install removes
  the quarantine attribute so macOS allows execution. This is the standard
  approach for early-access tools.
