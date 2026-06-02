# photohelper — Linux Installation

## Requirements

- Linux x86_64 with glibc 2.17+ (Ubuntu 20.04+, Debian 11+, Fedora 35+, etc.)
- No other dependencies — LibRaw and all required C libraries are statically linked

## Install

```bash
# 1. Untar
tar xzf photohelper-VERSION-x86_64-unknown-linux-gnu.tar.gz
cd photohelper-VERSION-x86_64-unknown-linux-gnu

# 2. Copy directory to a permanent location
mkdir -p ~/opt/photohelper
cp -R . ~/opt/photohelper/
chmod +x ~/opt/photohelper/photohelper.sh

# 3. Symlink the wrapper script to PATH
sudo ln -sf ~/opt/photohelper/photohelper.sh /usr/local/bin/photohelper

# OR: run directly from the archive directory
./photohelper.sh --help
```

## Why the wrapper script?

The archive includes `photohelper.sh` which sets `LD_LIBRARY_PATH` so the OS finds
`libonnxruntime.so` (the AI runtime) next to the binary. Use `photohelper.sh`
instead of `photohelper` directly.

## Verify

```bash
photohelper --help
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
