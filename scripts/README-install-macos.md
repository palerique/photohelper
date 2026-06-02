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
xattr -dr com.apple.quarantine photohelper

# 3. Copy binary and ORT runtime to system paths
sudo cp photohelper /usr/local/bin/
sudo cp libonnxruntime.dylib /usr/local/lib/

# 4. Place models in a permanent location
mkdir -p ~/photohelper/models
cp models/*.onnx   ~/photohelper/models/
cp models/manifest.toml ~/photohelper/models/

# 5. Add to your shell profile (~/.zshrc or ~/.bashrc)
echo 'export PHOTOHELPER_MODEL_DIR="$HOME/photohelper/models"' >> ~/.zshrc
source ~/.zshrc
```

## Verify

```bash
photohelper --help
photohelper --version
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
