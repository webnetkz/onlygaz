# OnlyGAZ source backup and recovery

This repository contains the Rust source, the complete runtime `assets/` tree
(including the Bugatti glTF, binary meshes, textures, audio and music), original
asset materials in `.asset_sources/`, license notices, documentation, and the
Microsoft Store artwork in `dist/OnlyGAZ_StoreAssets/`.

`target/` is a Cargo build cache. The other `dist/` contents are generated
MSIX files and staging copies of `assets/`; they are intentionally not committed
to Git. A Store package manifest is kept at
`packaging/windows/AppxManifest.xml`.

After cloning, check that `assets/models/bugatti/scene.gltf` and `scene.bin`
exist. Then install Rust 1.95 or newer and build from the repository root:

```powershell
cargo test --lib
cargo build --release
```

For a portable Windows build, copy `target/release/veyron_sim.exe` to a
distribution folder alongside the entire `assets/` directory. The executable
must not be distributed on its own.

To create a Store MSIX on Windows, first update the manifest version to one
greater than the previous Store submission, then prepare a staging folder:

```powershell
New-Item -ItemType Directory -Force dist\OnlyGAZ_MSIX_x64\Assets
Copy-Item target\release\veyron_sim.exe dist\OnlyGAZ_MSIX_x64\OnlyGAZ.exe
Copy-Item assets\* dist\OnlyGAZ_MSIX_x64\Assets -Recurse
Copy-Item dist\OnlyGAZ_StoreAssets\StoreLogo.png dist\OnlyGAZ_MSIX_x64\Assets
Copy-Item dist\OnlyGAZ_StoreAssets\Square150x150Logo.png dist\OnlyGAZ_MSIX_x64\Assets
Copy-Item dist\OnlyGAZ_StoreAssets\Square44x44Logo.png dist\OnlyGAZ_MSIX_x64\Assets
Copy-Item packaging\windows\AppxManifest.xml dist\OnlyGAZ_MSIX_x64
```

Use the Windows SDK `MakeAppx.exe pack /d dist\OnlyGAZ_MSIX_x64 /p
dist\OnlyGAZ_x64.msix` in a Developer Command Prompt, then submit the package
under the existing OnlyGAZ product identity. Microsoft Store signs and
distributes the accepted package.

The code's MIT/Apache license does **not** override the separate licenses of
third-party assets. Read `THIRD_PARTY_LICENSES.md` and the license files beside
individual models, textures, and audio before reusing or redistributing them.
