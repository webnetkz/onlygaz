//! Resolve the asset tree independently of the process working directory.
//!
//! MSIX desktop applications may start with a working directory other than
//! their installation directory.  The packaged asset tree is named `Assets`
//! and lives next to `OnlyGAZ.exe`.

use std::path::{Path, PathBuf};

#[cfg_attr(target_os = "android", allow(dead_code))]
fn find_asset_directory(
    executable_directory: Option<&Path>,
    working_directory: Option<&Path>,
    source_directory: &Path,
) -> Option<PathBuf> {
    // The executable's directory takes precedence over an unrelated working
    // directory, including one that happens to contain another `assets` tree.
    [
        executable_directory,
        working_directory,
        Some(source_directory),
    ]
    .into_iter()
    .flatten()
    .flat_map(|directory| [directory.join("Assets"), directory.join("assets")])
    .find(|candidate| candidate.is_dir())
}

/// On Android the asset tree is packed inside the APK and read through the
/// `AssetManager`; Bevy's Android asset reader ignores this root.
#[cfg(target_os = "android")]
pub fn asset_directory() -> PathBuf {
    PathBuf::from("assets")
}

#[cfg(not(target_os = "android"))]
pub fn asset_directory() -> PathBuf {
    let executable_directory = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let working_directory = std::env::current_dir().ok();
    let source_directory = Path::new(env!("CARGO_MANIFEST_DIR"));

    find_asset_directory(
        executable_directory.as_deref(),
        working_directory.as_deref(),
        source_directory,
    )
    .unwrap_or_else(|| {
        panic!(
            "OnlyGAZ assets are missing beside the executable, in the working directory, \
             and in the source tree"
        )
    })
}

/// Whether `relative` (a path inside the asset tree, e.g.
/// `models/bugatti/scene.gltf`) is present in the installed game.
#[cfg(not(target_os = "android"))]
pub fn asset_exists(relative: &str) -> bool {
    asset_directory().join(relative).is_file()
}

/// Android variant: look the file up inside the APK's `assets/` folder.
#[cfg(target_os = "android")]
pub fn asset_exists(relative: &str) -> bool {
    let Some(app) = bevy::android::ANDROID_APP.get() else {
        return false;
    };
    let Ok(path) = std::ffi::CString::new(relative) else {
        return false;
    };
    app.asset_manager().open(&path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_assets_win_over_the_working_directory() {
        let root = std::env::temp_dir().join(format!(
            "onlygaz-asset-path-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let packaged = root.join("package");
        let working = root.join("working");
        let source = root.join("source");
        std::fs::create_dir_all(packaged.join("Assets/models/bugatti")).unwrap();
        std::fs::write(packaged.join("Assets/models/bugatti/scene.gltf"), b"{}").unwrap();
        std::fs::create_dir_all(working.join("assets")).unwrap();
        std::fs::create_dir_all(source.join("assets")).unwrap();

        assert_eq!(
            find_asset_directory(Some(&packaged), Some(&working), &source),
            Some(packaged.join("Assets"))
        );
        assert!(
            find_asset_directory(Some(&packaged), Some(&working), &source)
                .unwrap()
                .join("models/bugatti/scene.gltf")
                .is_file()
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}
