//! One-time graphics preset selection from the machine's available resources.
//!
//! The phone build chooses between its two lightest presets
//! ([`recommended_mobile_quality`]); the desktop build between the other three
//! ([`recommended_quality`]).
#![cfg_attr(feature = "mobile", allow(dead_code))]

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;

use bevy::render::renderer::RenderAdapterInfo;
use wgpu_types::DeviceType;

use crate::world::lighting::LightingQuality;

#[cfg(any(target_os = "linux", target_os = "android"))]
use std::fs;

const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GpuKind {
    Integrated,
    Discrete,
    Virtual,
    Software,
    Unknown,
}

#[derive(Debug, Clone)]
pub(super) struct HardwareProfile {
    pub total_memory_bytes: Option<u64>,
    pub video_memory_bytes: Option<u64>,
    pub logical_cpus: usize,
    pub gpu_kind: GpuKind,
    pub gpu_name: String,
    pub device_model: String,
}

impl HardwareProfile {
    pub(super) fn detect(adapter: Option<&RenderAdapterInfo>) -> Self {
        let (gpu_kind, gpu_name) = adapter.map_or_else(
            || (GpuKind::Unknown, "unknown adapter".to_owned()),
            |adapter| {
                let kind = match adapter.device_type {
                    DeviceType::IntegratedGpu => GpuKind::Integrated,
                    DeviceType::DiscreteGpu => GpuKind::Discrete,
                    DeviceType::VirtualGpu => GpuKind::Virtual,
                    DeviceType::Cpu => GpuKind::Software,
                    DeviceType::Other => GpuKind::Unknown,
                };
                (kind, adapter.name.clone())
            },
        );
        Self {
            total_memory_bytes: total_memory_bytes(),
            video_memory_bytes: video_memory_bytes(),
            logical_cpus: std::thread::available_parallelism().map_or(1, usize::from),
            gpu_kind,
            gpu_name,
            device_model: device_model(),
        }
    }

    pub(super) fn description(&self) -> String {
        format!(
            "device=\"{}\", gpu=\"{}\" ({:?}), ram={}, vram={}, cpu_threads={}",
            self.device_model,
            self.gpu_name,
            self.gpu_kind,
            format_gib(self.total_memory_bytes),
            format_gib(self.video_memory_bytes),
            self.logical_cpus,
        )
    }
}

fn format_gib(bytes: Option<u64>) -> String {
    bytes.map_or_else(
        || "unknown".to_owned(),
        |bytes| format!("{:.1} GiB", bytes as f64 / GIB as f64),
    )
}

/// Choose conservatively when a resource cannot be measured. This prevents an
/// unsupported or software renderer from starting with the expensive deferred
/// path and gives unknown low-memory systems the minimum preset.
pub(super) fn recommended_quality(profile: &HardwareProfile) -> LightingQuality {
    let Some(memory) = profile.total_memory_bytes else {
        return LightingQuality::Performance;
    };
    let gpu_name = profile.gpu_name.to_ascii_lowercase();
    let device_model = profile.device_model.to_ascii_lowercase();
    if is_thermally_limited_device(&device_model)
        || memory < 10 * GIB
        || profile.logical_cpus < 4
        || matches!(profile.gpu_kind, GpuKind::Software | GpuKind::Virtual)
        || is_known_low_end_gpu(&gpu_name)
    {
        return LightingQuality::Performance;
    }

    let enough_for_maximum =
        memory >= 16 * GIB && profile.logical_cpus >= 8 && is_high_end_gpu(profile, &gpu_name);
    if enough_for_maximum {
        return LightingQuality::Cinematic;
    }

    let real_gpu = matches!(profile.gpu_kind, GpuKind::Integrated | GpuKind::Discrete);
    if memory >= 12 * GIB && profile.logical_cpus >= 6 && real_gpu {
        LightingQuality::Balanced
    } else {
        LightingQuality::Performance
    }
}

// The desktop build never calls the phone rules (only its tests do).
#[cfg_attr(not(feature = "mobile"), allow(dead_code))]
/// Phones: Lite for budget hardware - under 7 GiB of memory (every 4 GB and
/// 6 GB phone) or an entry-level GPU - and Performance otherwise. A phone
/// whose memory cannot be read is treated as a budget phone.
pub(super) fn recommended_mobile_quality(profile: &HardwareProfile) -> LightingQuality {
    let Some(memory) = profile.total_memory_bytes else {
        return LightingQuality::Lite;
    };
    let gpu_name = profile.gpu_name.to_ascii_lowercase();
    if memory < 7 * GIB
        || profile.logical_cpus < 6
        || matches!(profile.gpu_kind, GpuKind::Software)
        || is_budget_mobile_gpu(&gpu_name)
    {
        LightingQuality::Lite
    } else {
        LightingQuality::Performance
    }
}

#[cfg_attr(not(feature = "mobile"), allow(dead_code))]
/// Entry-level and older phone GPUs: Mali-T and the small Mali-G cores,
/// PowerVR, and Adreno below the 650 class.
fn is_budget_mobile_gpu(name: &str) -> bool {
    if name.contains("powervr") || name.contains("mali-t") {
        return true;
    }
    if let Some(model) = number_after(name, "mali-g") {
        // G31-G78 (and the older G71/G72) are budget or several years old;
        // G610 and newer, and every Immortalis, are current.
        return model < 100;
    }
    if let Some(model) = number_after(name, "adreno") {
        return model < 650;
    }
    false
}

#[cfg_attr(not(feature = "mobile"), allow(dead_code))]
/// The first number following `marker` in `name`, e.g. 610 in
/// "adreno (tm) 610".
fn number_after(name: &str, marker: &str) -> Option<u32> {
    let rest = &name[name.find(marker)? + marker.len()..];
    let digits: String = rest
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

fn is_thermally_limited_device(model: &str) -> bool {
    // MacBook Air is fanless. Recent M-series chips can render the cinematic
    // preset briefly, but a sustained race throttles the shared CPU/GPU package.
    model.contains("macbook air")
}

fn is_known_low_end_gpu(name: &str) -> bool {
    [
        "swiftshader",
        "llvmpipe",
        "microsoft basic render",
        "intel hd graphics",
        "geforce gt ",
        "geforce mx",
        "radeon r5",
        "radeon r7",
    ]
    .iter()
    .any(|marker| name.contains(marker))
}

fn is_high_end_gpu(profile: &HardwareProfile, name: &str) -> bool {
    if profile
        .video_memory_bytes
        .is_some_and(|bytes| bytes >= 8 * GIB)
    {
        return true;
    }

    // Apple GPUs use unified system memory. The named generations below have
    // enough shader throughput for the project's deferred cinematic preset.
    if name.contains("apple") {
        return name.contains("m3")
            || name.contains("m4")
            || name.contains(" pro")
            || name.contains(" max")
            || name.contains(" ultra");
    }

    let known_high_end = [
        "rtx 2080", "rtx 3070", "rtx 3080", "rtx 3090", "rtx 4060", "rtx 4070", "rtx 4080",
        "rtx 4090", "rtx 50", "rx 6800", "rx 6900", "rx 6950", "rx 7700", "rx 7800", "rx 7900",
        "arc a770",
    ];
    known_high_end.iter().any(|marker| name.contains(marker))
}

#[cfg(target_os = "macos")]
fn total_memory_bytes() -> Option<u64> {
    command_number("/usr/sbin/sysctl", &["-n", "hw.memsize"])
}

#[cfg(target_os = "macos")]
fn device_model() -> String {
    command_text(
        "/usr/sbin/system_profiler",
        &["SPHardwareDataType", "-detailLevel", "mini"],
    )
    .and_then(|text| {
        text.lines()
            .find_map(|line| line.trim().strip_prefix("Model Name:"))
            .map(str::trim)
            .map(str::to_owned)
    })
    .or_else(|| command_text("/usr/sbin/sysctl", &["-n", "hw.model"]))
    .unwrap_or_else(|| "unknown model".to_owned())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn total_memory_bytes() -> Option<u64> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    let kib = text
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    kib.checked_mul(1024)
}

#[cfg(target_os = "linux")]
fn device_model() -> String {
    fs::read_to_string("/sys/devices/virtual/dmi/id/product_name")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown model".to_owned())
}

#[cfg(target_os = "windows")]
fn total_memory_bytes() -> Option<u64> {
    powershell_number("(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory")
}

#[cfg(target_os = "windows")]
fn device_model() -> String {
    powershell_text("(Get-CimInstance Win32_ComputerSystem).Model")
        .unwrap_or_else(|| "unknown model".to_owned())
}

/// Android system property, such as `ro.product.model`.
#[cfg(target_os = "android")]
fn system_property(name: &str) -> Option<String> {
    use std::ffi::{CStr, CString, c_char, c_int};

    unsafe extern "C" {
        // Bionic libc; PROP_VALUE_MAX is 92 bytes including the terminator.
        fn __system_property_get(name: *const c_char, value: *mut c_char) -> c_int;
    }

    let name = CString::new(name).ok()?;
    let mut value = [0 as c_char; 92];
    // SAFETY: `name` is NUL-terminated and `value` has the PROP_VALUE_MAX
    // bytes the function may write, including the terminator.
    let length = unsafe { __system_property_get(name.as_ptr(), value.as_mut_ptr()) };
    if length <= 0 {
        return None;
    }
    // SAFETY: on success the buffer holds a NUL-terminated string.
    let text = unsafe { CStr::from_ptr(value.as_ptr()) }
        .to_string_lossy()
        .trim()
        .to_owned();
    (!text.is_empty()).then_some(text)
}

#[cfg(target_os = "android")]
fn device_model() -> String {
    let model = [
        system_property("ro.product.manufacturer"),
        system_property("ro.product.model"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    let chip = system_property("ro.soc.model").or_else(|| system_property("ro.board.platform"));
    match (model.is_empty(), chip) {
        (true, None) => "unknown model".to_owned(),
        (true, Some(chip)) => chip,
        (false, None) => model,
        (false, Some(chip)) => format!("{model} ({chip})"),
    }
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "windows",
    target_os = "android"
)))]
fn total_memory_bytes() -> Option<u64> {
    None
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "windows",
    target_os = "android"
)))]
fn device_model() -> String {
    "unknown model".to_owned()
}

#[cfg(target_os = "linux")]
fn video_memory_bytes() -> Option<u64> {
    let entries = fs::read_dir("/sys/class/drm").ok()?;
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            fs::read_to_string(entry.path().join("device/mem_info_vram_total"))
                .ok()?
                .trim()
                .parse::<u64>()
                .ok()
        })
        .max()
}

#[cfg(target_os = "windows")]
fn video_memory_bytes() -> Option<u64> {
    powershell_number(
        "(Get-CimInstance Win32_VideoController | Measure-Object AdapterRAM -Maximum).Maximum",
    )
}

// Apple GPUs use the system memory pool, so a separate VRAM number would be
// misleading. Other unsupported targets use the GPU-name fallback.
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn video_memory_bytes() -> Option<u64> {
    None
}

#[cfg(target_os = "macos")]
fn command_number(program: &str, arguments: &[&str]) -> Option<u64> {
    command_text(program, arguments)?.parse().ok()
}

#[cfg(target_os = "macos")]
fn command_text(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program).args(arguments).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(target_os = "windows")]
fn powershell_number(expression: &str) -> Option<u64> {
    powershell_text(expression)?.parse().ok()
}

#[cfg(target_os = "windows")]
fn powershell_text(expression: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", expression])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(
        memory_gib: u64,
        logical_cpus: usize,
        gpu_kind: GpuKind,
        gpu_name: &str,
        video_memory_gib: Option<u64>,
        device_model: &str,
    ) -> HardwareProfile {
        HardwareProfile {
            total_memory_bytes: Some(memory_gib * GIB),
            video_memory_bytes: video_memory_gib.map(|value| value * GIB),
            logical_cpus,
            gpu_kind,
            gpu_name: gpu_name.to_owned(),
            device_model: device_model.to_owned(),
        }
    }

    #[test]
    fn weak_or_unmeasurable_hardware_uses_the_minimum_preset() {
        let weak = profile(
            8,
            4,
            GpuKind::Integrated,
            "Intel UHD Graphics",
            None,
            "Budget Laptop",
        );
        assert_eq!(recommended_quality(&weak), LightingQuality::Performance);

        let unknown = HardwareProfile {
            total_memory_bytes: None,
            ..weak
        };
        assert_eq!(recommended_quality(&unknown), LightingQuality::Performance);
    }

    #[test]
    fn capable_mainstream_hardware_uses_the_balanced_preset() {
        let profile = profile(16, 8, GpuKind::Integrated, "Apple M1", None, "MacBook Pro");
        assert_eq!(recommended_quality(&profile), LightingQuality::Balanced);
    }

    #[test]
    fn macbook_air_always_uses_the_minimum_preset() {
        let profile = profile(32, 10, GpuKind::Integrated, "Apple M4", None, "MacBook Air");
        assert_eq!(recommended_quality(&profile), LightingQuality::Performance);
    }

    #[test]
    fn budget_phones_use_lite_and_capable_phones_performance() {
        let phone = |memory_gib, gpu: &str| HardwareProfile {
            total_memory_bytes: Some(memory_gib * GIB - GIB / 4),
            video_memory_bytes: None,
            logical_cpus: 8,
            gpu_kind: GpuKind::Integrated,
            gpu_name: gpu.to_owned(),
            device_model: "Phone".to_owned(),
        };
        for (memory, gpu) in [
            (4, "Mali-G52 MC2"),
            (4, "Adreno (TM) 610"),
            (6, "Adreno (TM) 730"),
            (8, "Mali-G57 MC2"),
            (8, "Adreno (TM) 619"),
            (8, "PowerVR Rogue GE8320"),
            (12, "Mali-T880"),
        ] {
            assert_eq!(
                recommended_mobile_quality(&phone(memory, gpu)),
                LightingQuality::Lite,
                "{memory} GB {gpu}"
            );
        }
        for (memory, gpu) in [
            (8, "Adreno (TM) 730"),
            (8, "Mali-G610 MC6"),
            (12, "Immortalis-G715"),
            (8, "Samsung Xclipse 920"),
        ] {
            assert_eq!(
                recommended_mobile_quality(&phone(memory, gpu)),
                LightingQuality::Performance,
                "{memory} GB {gpu}"
            );
        }
        let unknown = HardwareProfile {
            total_memory_bytes: None,
            ..phone(8, "Adreno (TM) 740")
        };
        assert_eq!(recommended_mobile_quality(&unknown), LightingQuality::Lite);
    }

    #[test]
    fn modern_high_end_hardware_uses_the_maximum_preset() {
        for profile in [
            profile(16, 10, GpuKind::Integrated, "Apple M4", None, "Mac mini"),
            profile(
                32,
                12,
                GpuKind::Discrete,
                "NVIDIA GeForce RTX 4070",
                Some(12),
                "Gaming Desktop",
            ),
        ] {
            assert_eq!(recommended_quality(&profile), LightingQuality::Cinematic);
        }
    }
}
