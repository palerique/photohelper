# photohelper — macOS Installation

## Requirements

- macOS 13 (Ventura) or later, Apple Silicon (arm64)
- Intel Macs: run the arm64 binary via Rosetta 2 — no configuration needed

## Install

```bash
# 1. Untar
tar xzf photohelper-VERSION-aarch64-apple-darwin.tar.gz
cd photohelper-VERSION-aarch64-apple-darwin

# 2. Allow macOS to run the binary (v0.1 is not yet notarized)
xattr -dr com.apple.quarantine photohelper

# 3. Copy binary to system PATH
sudo cp photohelper /usr/local/bin/

# 4. Place models in a permanent location
mkdir -p ~/photohelper/models
cp models/*.onnx        ~/photohelper/models/
cp models/manifest.toml ~/photohelper/models/

# 5. Add model directory to your shell profile (~/.zshrc or ~/.bashrc)
echo 'export PHOTOHELPER_MODEL_DIR="$HOME/photohelper/models"' >> ~/.zshrc
source ~/.zshrc

# 6. Verify
photohelper --help
```

## Notes

- **No extra libraries needed.** On Apple Silicon, ORT inference runs natively
  through Apple's CoreML framework (built into macOS). The binary is self-contained.

- `PHOTOHELPER_MODEL_DIR` is only required for the `cull` and `dedup` subcommands
  (AI features). All other subcommands work without it.

- v0.1 binaries are not code-signed. Run step 2 to bypass Gatekeeper.

## Uninstall

```bash
sudo rm /usr/local/bin/photohelper
rm -rf ~/photohelper
```
