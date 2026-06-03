# photohelper — Windows Installation

## Requirements

- Windows 10 or later (x86_64)
- No additional software required — all dependencies are bundled

## Install

```powershell
# 1. Unzip the archive (Windows 10+ can open .zip natively,
#    or use 7-Zip / Windows Explorer → Extract All)
Expand-Archive photohelper-VERSION-x86_64-pc-windows-msvc.zip -DestinationPath .
cd photohelper-VERSION-x86_64-pc-windows-msvc

# 2. Copy files to a permanent location
New-Item -ItemType Directory -Force -Path "$env:LOCALAPPDATA\photohelper\models"
Copy-Item photohelper.exe -Destination "$env:LOCALAPPDATA\photohelper\"
Copy-Item models\* -Destination "$env:LOCALAPPDATA\photohelper\models\" -Recurse

# 3. Add to PATH and set model directory in your PowerShell profile
Add-Content $PROFILE @'
$env:PATH += ";$env:LOCALAPPDATA\photohelper"
$env:PHOTOHELPER_MODEL_DIR = "$env:LOCALAPPDATA\photohelper\models"
'@

# 4. Reload your profile
. $PROFILE

# 5. Verify
photohelper --help
```

## Alternative: manual PATH setup

If you prefer not to modify your profile, set the environment variables
before each use in a terminal session:

```powershell
$env:PATH += ";C:\path\to\photohelper"
$env:PHOTOHELPER_MODEL_DIR = "C:\path\to\photohelper\models"
photohelper --help
```

## Uninstall

```powershell
Remove-Item -Recurse "$env:LOCALAPPDATA\photohelper"
# Also remove the lines added to $PROFILE if desired
```

## Notes

- `PHOTOHELPER_MODEL_DIR` only needs to be set for the `cull` and `dedup`
  subcommands (AI features). All other subcommands work without it.

- This is an MSVC-compiled native Windows binary; ORT is statically linked
  (no additional DLLs required). The binary links against standard Windows 10+
  system DLLs (DirectML, DXGI, D3D12) which are always present.
