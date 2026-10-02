//! Android platform glue: the motion sensor, the display rotation and
//! display cut-out insets.
//!
//! * The gravity sensor (or the raw accelerometer on phones without one) is
//!   read on a small dedicated thread with its own `ALooper`, so the game
//!   thread never blocks on sensor I/O. The sensor only runs while tilt
//!   steering is in use, to save battery.
//! * Sensors report in the device's natural (portrait) axes. The game runs in
//!   landscape and may be turned over by 180°, so readings are rotated into
//!   screen axes with the current display rotation.
//! * `MainActivity` (android/app/src/main/java) reports the rotation and
//!   publishes the safe-area insets of display cut-outs; both are read here
//!   through JNI.
//! * Panic messages go to logcat (Android drops stderr), so a crash shows up
//!   in Android Studio's Logcat window with its message and source location.

use std::ffi::c_int;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use ndk_sys as ffi;

use super::layout::SafeArea;

/// Release package name; the debug build adds `.debug` (see
/// [`package_name`]).
const PACKAGE_NAME: &std::ffi::CStr = c"kz.webnet.onlygaz";
/// Logcat tag of the game's own messages.
const LOG_TAG: &std::ffi::CStr = c"OnlyGAZ";
/// Sensor sampling period: 60 Hz is plenty for steering.
const SAMPLING_PERIOD_US: i32 = 16_667;
/// A reading older than this is treated as "no sensor data".
const STALE_AFTER: Duration = Duration::from_millis(500);

#[derive(Default)]
struct SensorShared {
    wanted: AtomicBool,
    running: AtomicBool,
    x: AtomicU32,
    y: AtomicU32,
    z: AtomicU32,
    /// Milliseconds since `EPOCH` of the newest sample, 0 = none.
    stamp_ms: AtomicU64,
}

static SENSOR: OnceLock<Arc<SensorShared>> = OnceLock::new();
static EPOCH: OnceLock<Instant> = OnceLock::new();
/// `Display.getRotation()`: 0, 1 (90°), 2 (180°) or 3 (270°). Landscape
/// phones start in 90°.
static DISPLAY_ROTATION: AtomicU32 = AtomicU32::new(1);

/// Write panics to logcat as well: Android discards the standard error
/// stream the default hook prints to. The previous hook still runs.
pub fn log_panics_to_logcat() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = format!("{info}").replace('\0', " ");
        if let Ok(message) = std::ffi::CString::new(message) {
            // SAFETY: both pointers are valid NUL-terminated strings for the
            // duration of the call.
            unsafe {
                ffi::__android_log_write(
                    ffi::android_LogPriority::ANDROID_LOG_FATAL.0 as c_int,
                    LOG_TAG.as_ptr(),
                    message.as_ptr(),
                );
            }
        }
        previous(info);
    }));
}

/// The running app's package name: `kz.webnet.onlygaz`, or
/// `kz.webnet.onlygaz.debug` for the Android Studio debug build. The sensor
/// service checks it against the app, so it is read from the process name
/// (`/proc/self/cmdline`, which Android sets to the package name).
fn package_name() -> std::ffi::CString {
    std::fs::read("/proc/self/cmdline")
        .ok()
        .and_then(|bytes| {
            let name = bytes.split(|byte| *byte == 0).next()?.to_vec();
            let looks_like_package = !name.is_empty()
                && name.contains(&b'.')
                && !name.contains(&b'/')
                && !name.contains(&b':');
            if looks_like_package {
                std::ffi::CString::new(name).ok()
            } else {
                None
            }
        })
        .unwrap_or_else(|| PACKAGE_NAME.to_owned())
}

fn now_ms() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

/// Latest gravity reading in screen axes [m/s²], or `None` when there is no
/// fresh data. `wanted` switches the sensor on or off.
pub fn gravity(wanted: bool) -> Option<Vec3> {
    let shared = SENSOR.get_or_init(|| {
        let shared = Arc::new(SensorShared::default());
        let thread_shared = shared.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("onlygaz-tilt".into())
            .spawn(move || sensor_thread(thread_shared))
        {
            warn!("tilt sensor thread could not start: {error}");
        }
        shared
    });
    shared.wanted.store(wanted, Ordering::Relaxed);
    if !wanted || !shared.running.load(Ordering::Relaxed) {
        return None;
    }
    let stamp = shared.stamp_ms.load(Ordering::Acquire);
    if stamp == 0 || now_ms().saturating_sub(stamp) > STALE_AFTER.as_millis() as u64 {
        return None;
    }
    let reading = Vec3::new(
        f32::from_bits(shared.x.load(Ordering::Relaxed)),
        f32::from_bits(shared.y.load(Ordering::Relaxed)),
        f32::from_bits(shared.z.load(Ordering::Relaxed)),
    );
    Some(super::tilt::device_to_screen(
        reading,
        DISPLAY_ROTATION.load(Ordering::Relaxed),
    ))
}

fn sensor_thread(shared: Arc<SensorShared>) {
    // SAFETY: plain NDK calls on a thread that owns the looper it prepares.
    // Every pointer is checked before use and stays valid for the thread's
    // lifetime (the sensor manager and sensors are process singletons).
    unsafe {
        let looper = ffi::ALooper_prepare(ffi::ALOOPER_PREPARE_ALLOW_NON_CALLBACKS as c_int);
        let package = package_name();
        let manager = ffi::ASensorManager_getInstanceForPackage(package.as_ptr());
        if looper.is_null() || manager.is_null() {
            warn!("tilt steering: no sensor manager");
            return;
        }
        let mut sensor =
            ffi::ASensorManager_getDefaultSensor(manager, ffi::ASENSOR_TYPE_GRAVITY as c_int);
        // The raw accelerometer also contains hand movement; filter it harder.
        let mut low_pass = 1.0_f32;
        if sensor.is_null() {
            sensor = ffi::ASensorManager_getDefaultSensor(
                manager,
                ffi::ASENSOR_TYPE_ACCELEROMETER as c_int,
            );
            low_pass = 0.25;
        }
        if sensor.is_null() {
            warn!("tilt steering: this device has no accelerometer");
            return;
        }
        let queue =
            ffi::ASensorManager_createEventQueue(manager, looper, 1, None, std::ptr::null_mut());
        if queue.is_null() {
            warn!("tilt steering: sensor event queue unavailable");
            return;
        }
        info!(
            "tilt steering: using the {} sensor",
            if low_pass < 1.0 {
                "accelerometer"
            } else {
                "gravity"
            }
        );

        let mut enabled = false;
        let mut filtered = Vec3::ZERO;
        let mut have_sample = false;
        let mut events: [ffi::ASensorEvent; 16] = std::mem::zeroed();
        loop {
            let wanted = shared.wanted.load(Ordering::Relaxed);
            if wanted && !enabled {
                enabled =
                    ffi::ASensorEventQueue_registerSensor(queue, sensor, SAMPLING_PERIOD_US, 0)
                        == 0;
                have_sample = false;
                shared.running.store(enabled, Ordering::Relaxed);
            } else if !wanted && enabled {
                ffi::ASensorEventQueue_disableSensor(queue, sensor);
                enabled = false;
                shared.running.store(false, Ordering::Relaxed);
                shared.stamp_ms.store(0, Ordering::Release);
            }
            if !enabled {
                std::thread::sleep(Duration::from_millis(120));
                continue;
            }

            ffi::ALooper_pollOnce(
                100,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            loop {
                let count =
                    ffi::ASensorEventQueue_getEvents(queue, events.as_mut_ptr(), events.len());
                if count <= 0 {
                    break;
                }
                for event in &events[..count as usize] {
                    let data = event.__bindgen_anon_1.__bindgen_anon_1.data;
                    let reading = Vec3::new(data[0], data[1], data[2]);
                    if !reading.is_finite() {
                        continue;
                    }
                    filtered = if have_sample {
                        filtered + (reading - filtered) * low_pass
                    } else {
                        reading
                    };
                    have_sample = true;
                }
                shared.x.store(filtered.x.to_bits(), Ordering::Relaxed);
                shared.y.store(filtered.y.to_bits(), Ordering::Relaxed);
                shared.z.store(filtered.z.to_bits(), Ordering::Relaxed);
                shared.stamp_ms.store(now_ms(), Ordering::Release);
            }
        }
    }
}

/// What `MainActivity` reports about the display.
struct DisplayInfo {
    /// Cut-out safe insets in physical pixels: left, top, right, bottom.
    insets: [i32; 4],
    rotation: u32,
}

/// Read the display rotation and cut-out insets from
/// `kz.webnet.onlygaz.MainActivity`.
fn read_display_info() -> Option<DisplayInfo> {
    let app = bevy::android::ANDROID_APP.get()?;
    // SAFETY: the VM and activity pointers come from android-activity and stay
    // valid while the activity exists; the activity reference is never freed
    // here.
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) }.ok()?;
    let mut env = vm.attach_current_thread_permanently().ok()?;
    let activity =
        unsafe { jni::objects::JObject::from_raw(app.activity_as_ptr() as jni::sys::jobject) };
    let result = env.with_local_frame(8, |env| -> jni::errors::Result<DisplayInfo> {
        let rotation = env
            .call_method(&activity, "currentDisplayRotation", "()I", &[])?
            .i()?;
        let class = env.get_object_class(&activity)?;
        let mut field = |name: &str| -> jni::errors::Result<i32> {
            env.get_static_field(&class, name, "I")?.i()
        };
        let insets = [
            field("safeInsetLeft")?,
            field("safeInsetTop")?,
            field("safeInsetRight")?,
            field("safeInsetBottom")?,
        ];
        Ok(DisplayInfo {
            insets,
            rotation: rotation.clamp(0, 3) as u32,
        })
    });
    match result {
        Ok(info) => Some(info),
        Err(_) => {
            // A missing method or field (e.g. a plain NativeActivity) leaves a
            // pending Java exception that would break every later JNI call.
            let _ = env.exception_clear();
            None
        }
    }
}

/// Refresh the display rotation (for tilt steering) and [`SafeArea`] twice a
/// second. Turning a landscape phone over by 180° changes no configuration,
/// so the rotation has to be polled.
pub fn poll_display_info(
    time: Res<Time<Real>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut safe_area: ResMut<SafeArea>,
    mut next_poll: Local<f32>,
    mut unavailable: Local<bool>,
) {
    if *unavailable || time.elapsed_secs() < *next_poll {
        return;
    }
    *next_poll = time.elapsed_secs() + 0.5;
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(info) = read_display_info() else {
        *unavailable = true;
        warn!("display rotation / cut-out insets unavailable; assuming landscape, fixed margins");
        return;
    };
    let previous = DISPLAY_ROTATION.swap(info.rotation, Ordering::Relaxed);
    if previous != info.rotation {
        info!("display rotation {}°", info.rotation * 90);
    }
    let [left, top, right, bottom] = info.insets;
    let scale = window.scale_factor().max(0.1);
    let next = SafeArea {
        top: top.max(0) as f32 / scale,
        bottom: bottom.max(0) as f32 / scale,
        left: left.max(0) as f32 / scale,
        right: right.max(0) as f32 / scale,
    };
    if *safe_area != next {
        info!(
            "safe area (logical px): top {:.0}, bottom {:.0}, left {:.0}, right {:.0}",
            next.top, next.bottom, next.left, next.right
        );
        *safe_area = next;
    }
}
