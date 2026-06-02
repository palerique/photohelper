# photohelper — Linux Installation

## Requirements

- Linux x86_64 with glibc 2.17+ (Ubuntu 20.04+, Debian 11+, Fedora 35+, etc.)
- No other dependencies — LibRaw and all required C libraries are statically linked

## Install

```bash
# 1. Untar
tar xzf photohelper-VERSION-x86_64-unknown-linux-gnu.tar.gz
cd photohelper-VERSION-x86_64-unknown-linux-gnu

# 2. Copy binary and ORT runtime
sudo cp photohelper /usr/local/bin/
sudo cp libonnxruntime.so* /usr/local/lib/
sudo ldconfig

# 3. Place models in a permanent location
mkdir -p ~/photohelper/models
cp models/*.onnx        ~/photohelper/models/
cp models/manifest.toml ~/photohelper/models/

# 4. Add to your shell profile (~/.bashrc or ~/.zshrc)
echo 'export PHOTOHELPER_MODEL_DIR="$HOME/photohelper/models"' >> ~/.bashrc
source ~/.bashrc
```

## Verify

```bash
photohelper --help
photohelper --version
```

## Uninstall

```bash
sudo rm /usr/local/bin/photohelper
sudo rm /usr/local/lib/libonnxruntime.so*
sudo ldconfig
rm -rf ~/photohelper
```

## Notes

- The `PHOTOHELPER_MODEL_DIR` environment variable must point to the `models/`
  directory at runtime. The AI models (NIMA + CLIP) are required for `cull`
  and `dedup` subcommands only. All other subcommands work without setting
  this variable.

- This build is dynamically linked against glibc (the standard Linux C library).
  It works on any modern Linux distribution. Static musl builds are planned
  for v0.2 (see TD-025 in TECH-DEBT.md).
