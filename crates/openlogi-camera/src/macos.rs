//! AVFoundation-backed camera enumeration.
//!
//! `+[AVCaptureDevice devicesWithMediaType:AVMediaTypeVideo]` returns every
//! video-capable capture device macOS knows about; for each we read the same
//! `localizedName` / `uniqueID` / `modelID` strings `system_profiler
//! SPCameraDataType` reports. Vendor parsing + the Logitech filter live in the
//! platform-independent parent module, so this file is purely "what cameras
//! does the OS see".

#![expect(
    unsafe_code,
    reason = "AVFoundation (AVCaptureDevice) camera-enumeration FFI"
)]

use std::ffi::CStr;
use std::os::raw::c_char;

use objc::runtime::{Class, Object};
use objc::{msg_send, sel, sel_impl};

/// Raw camera metadata as reported by `AVCaptureDevice`, before vendor parsing.
pub(crate) struct RawCamera {
    pub name: String,
    pub unique_id: String,
    pub model_id: String,
}

// `AVMediaTypeVideo` is an `NSString` constant exported by AVFoundation; the
// framework must be linked for it and the `AVCaptureDevice` class to resolve.
#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeVideo: *const Object;
}

/// Enumerate every video `AVCaptureDevice`, as raw metadata. The Logitech
/// filter is applied by the caller in `lib.rs`.
pub(crate) fn enumerate() -> Vec<RawCamera> {
    let (Some(pool_cls), Some(device_cls)) = (
        Class::get("NSAutoreleasePool"),
        Class::get("AVCaptureDevice"),
    ) else {
        return Vec::new();
    };

    // SAFETY: `NSAutoreleasePool` / `AVCaptureDevice` exist once AVFoundation is
    // linked. Every message uses a documented selector and matching types; the
    // returned array + its devices are autoreleased, so an explicit pool brackets
    // the work and every string is copied into an owned `String` before it drains.
    unsafe {
        let pool: *mut Object = msg_send![pool_cls, new];
        let devices: *mut Object = msg_send![device_cls, devicesWithMediaType: AVMediaTypeVideo];

        let mut out = Vec::new();
        if !devices.is_null() {
            let count: usize = msg_send![devices, count];
            out.reserve(count);
            for i in 0..count {
                let device: *mut Object = msg_send![devices, objectAtIndex: i];
                if device.is_null() {
                    continue;
                }
                let name_obj: *mut Object = msg_send![device, localizedName];
                let uid_obj: *mut Object = msg_send![device, uniqueID];
                let model_obj: *mut Object = msg_send![device, modelID];
                if let (Some(name), Some(unique_id), Some(model_id)) =
                    (nsstring(name_obj), nsstring(uid_obj), nsstring(model_obj))
                {
                    out.push(RawCamera {
                        name,
                        unique_id,
                        model_id,
                    });
                }
            }
        }

        let _: () = msg_send![pool, drain];
        out
    }
}

/// Copy an `NSString` into an owned Rust `String`. `None` for a null pointer or
/// non-UTF-8 contents.
fn nsstring(s: *mut Object) -> Option<String> {
    if s.is_null() {
        return None;
    }
    // SAFETY: `s` is a non-null `NSString`; `UTF8String` yields a NUL-terminated
    // C string valid for the lifetime of the (autoreleased) string, which we
    // copy out of immediately.
    unsafe {
        let utf8: *const c_char = msg_send![s, UTF8String];
        if utf8.is_null() {
            return None;
        }
        Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }
}
