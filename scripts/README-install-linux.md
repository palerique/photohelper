# photohelper — Linux Installation

## Requirements

- Linux x86_64 with glibc 2.17+ (Ubuntu 20.04+, Debian 11+, Fedora 35+, etc.)
- No extra runtime libraries — ORT is statically linked; the binary is self-contained

## Install

```bash
# 1. Untar
tar xzf photohelper-VERSION-x86_64-unknown-linux-gnu.tar.gz
cd photohelper-VERSION-x86_64-unknown-linux-gnu

# 2. Copy binary to system PATH
sudo cp photohelper /usr/local/bin/

# 3. Place models in a permanent location
mkdir -p ~/photohelper/models
cp models/*.onnx        ~/photohelper/models/
cp models/manifest.toml ~/photohelper/models/

# 4. Add model directory to your shell profile (~/.bashrc or ~/.zshrc)
echo 'export PHOTOHELPER_MODEL_DIR="$HOME/photohelper/models"' >> ~/.bashrc
source ~/.bashrc

# 5. Verify
photohelper --help
```

## Notes

- **No extra libraries needed.** The ORT AI runtime is statically linked into the
  binary. The binary depends only on glibc + libstdc++ (standard on all Linux distros).

- `PHOTOHELPER_MODEL_DIR` is only required for `cull` and `dedup` (AI features).
  All other subcommands work without it.

## Uninstall

```bash
sudo rm /usr/local/bin/photohelper
rm -rf ~/photohelper
```
