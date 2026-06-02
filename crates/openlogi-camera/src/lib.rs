//! Generic discovery of Logitech USB Video Class (UVC) webcams.
//!
//! Mice and keyboards speak Logitech's proprietary HID++ (over a Bolt/Unifying
//! receiver or directly) — see the `openlogi-hid` crate. Webcams don't: every
//! Logitech camera (StreamCam, Brio, C920, C922, C270, C930e, …) is a standard
//! UVC device and enumerates the same way. So detection keys off the USB vendor
//! id (`0x046d`) rather than any per-model quirk — plug in *any* Logitech
//! camera and it's recognised, with no model table to maintain.
//!
//! macOS is the only platform with a real backend today (AVFoundation's
//! `AVCaptureDevice`); other platforms return an empty list.

use serde::Serialize;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
mod capture;
#[cfg(target_os = "macos")]
pub use capture::{CaptureError, Frame, capture_frame};

#[cfg(not(target_os = "macos"))]
mod capture {
    use std::time::Duration;

    /// One decoded camera frame, tightly-packed RGBA8.
    #[derive(Clone)]
    pub struct Frame {
        pub width: u32,
        pub height: u32,
        pub rgba: Vec<u8>,
    }

    /// Why a capture attempt failed.
    #[derive(Debug, Clone)]
    pub enum CaptureError {
        /// Capture has no backend on this platform.
        Unsupported,
    }

    impl std::fmt::Display for CaptureError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "camera capture is only implemented on macOS")
        }
    }

    impl std::error::Error for CaptureError {}

    /// Stub: returns [`CaptureError::Unsupported`] off macOS.
    pub fn capture_frame(_unique_id: &str, _timeout: Duration) -> Result<Frame, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}
#[cfg(not(target_os = "macos"))]
pub use capture::{CaptureError, Frame, capture_frame};

/// Logitech's USB vendor id. Reported in decimal (`1133`) inside an
/// `AVCaptureDevice` modelID, and in hex (`046d`) most everywhere else.
pub const LOGITECH_VID: u16 = 0x046d;

/// A connected USB Video Class camera.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Camera {
    /// Human-readable name, e.g. `"Logitech StreamCam"`.
    pub name: String,
    /// Stable per-device identifier from the OS capture layer. Keys the device
    /// in the UI so two identical cameras stay distinct.
    pub unique_id: String,
    /// USB vendor id (`0x046d` for Logitech).
    pub vendor_id: u16,
    /// USB product id (e.g. `0x0893` for the StreamCam).
    pub product_id: u16,
    /// Largest supported frame size `(width, height)`, when the OS reports the
    /// device's formats. Read from metadata only — no capture, no permission.
    pub max_resolution: Option<(u32, u32)>,
    /// Highest supported frame rate (fps) across all formats, when known.
    pub max_fps: Option<u32>,
}

/// Enumerate every connected **Logitech** UVC camera.
///
/// Non-Logitech cameras (the built-in FaceTime camera, virtual cameras, other
/// vendors' webcams) are filtered out. Returns an empty list on platforms with
/// no capture backend, or when no Logitech camera is attached.
#[must_use]
pub fn enumerate_cameras() -> Vec<Camera> {
    enumerate_all()
        .into_iter()
        .filter(|camera| camera.vendor_id == LOGITECH_VID)
        .collect()
}

#[cfg(target_os = "macos")]
fn enumerate_all() -> Vec<Camera> {
    macos::enumerate()
        .iter()
        .filter_map(|raw| {
            let mut camera = Camera::from_raw(&raw.name, &raw.unique_id, &raw.model_id)?;
            if raw.max_width > 0 && raw.max_height > 0 {
                camera.max_resolution = Some((raw.max_width, raw.max_height));
            }
            if raw.max_fps > 0 {
                camera.max_fps = Some(raw.max_fps);
            }
            Some(camera)
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn enumerate_all() -> Vec<Camera> {
    Vec::new()
}

impl Camera {
    /// Build a [`Camera`] from an OS-reported `(name, unique_id, model_id)`.
    ///
    /// Returns `None` when `model_id` carries no USB vendor/product id — i.e.
    /// it isn't a real USB camera (the macOS FaceTime camera's modelID is just
    /// `"FaceTime HD Camera"`), so it can't be attributed to a vendor and is
    /// dropped before the Logitech filter even runs. Format fields start `None`;
    /// the platform backend fills them in.
    fn from_raw(name: &str, unique_id: &str, model_id: &str) -> Option<Self> {
        let (vendor_id, product_id) = parse_vid_pid(model_id)?;
        Some(Self {
            name: name.to_string(),
            unique_id: unique_id.to_string(),
            vendor_id,
            product_id,
            max_resolution: None,
            max_fps: None,
        })
    }
}

/// Pull the USB vendor/product id out of an `AVCaptureDevice` modelID such as
/// `"UVC Camera VendorID_1133 ProductID_2195"`. Both ids are **decimal** in
/// that string (1133 == 0x046d, 2195 == 0x0893). `None` if either marker is
/// absent.
fn parse_vid_pid(model_id: &str) -> Option<(u16, u16)> {
    let vendor_id = parse_marker(model_id, "VendorID_")?;
    let product_id = parse_marker(model_id, "ProductID_")?;
    Some((vendor_id, product_id))
}

/// Read the decimal number immediately following `marker` in `haystack`.
fn parse_marker(haystack: &str, marker: &str) -> Option<u16> {
    let rest = haystack.split(marker).nth(1)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_logitech_streamcam_model_id() {
        assert_eq!(
            parse_vid_pid("UVC Camera VendorID_1133 ProductID_2195"),
            Some((0x046d, 0x0893))
        );
    }

    #[test]
    fn rejects_model_id_without_usb_ids() {
        assert_eq!(parse_vid_pid("FaceTime HD Camera"), None);
        assert_eq!(parse_vid_pid("VendorID_1133 only"), None);
    }

    #[test]
    fn from_raw_keeps_usb_cameras_and_drops_the_rest() {
        assert_eq!(
            Camera::from_raw(
                "Logitech StreamCam",
                "0x1123000046d0893",
                "UVC Camera VendorID_1133 ProductID_2195",
            ),
            Some(Camera {
                name: "Logitech StreamCam".to_string(),
                unique_id: "0x1123000046d0893".to_string(),
                vendor_id: LOGITECH_VID,
                product_id: 0x0893,
                max_resolution: None,
                max_fps: None,
            })
        );
        assert_eq!(
            Camera::from_raw("FaceTime HD Camera", "uuid", "FaceTime HD Camera"),
            None
        );
    }
}
