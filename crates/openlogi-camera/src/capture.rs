//! AVFoundation camera capture: a one-shot snapshot and a live frame stream.
//!
//! Both open an `AVCaptureSession` on the chosen camera and read BGRA frames
//! through an `AVCaptureVideoDataOutput` delegate, converting to RGBA. Capturing
//! (unlike enumeration) needs Camera permission *and* an app bundle carrying
//! `NSCameraUsageDescription`; from an unbundled binary macOS denies access,
//! which surfaces as [`CaptureError::AccessDenied`].

#![expect(
    unsafe_code,
    reason = "AVFoundation / CoreMedia / CoreVideo capture FFI"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "pixel dimensions and FourCC constants are bounded and copied verbatim"
)]

use std::ffi::{CString, c_void};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use block::ConcreteBlock;
use objc::declare::ClassDecl;
use objc::rc::StrongPtr;
use objc::runtime::{BOOL, Class, NO, Object, Sel};
use objc::{class, msg_send, sel, sel_impl};

/// One decoded camera frame, tightly-packed RGBA8 (`width * height * 4` bytes).
#[derive(Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Why a capture attempt failed.
#[derive(Debug, Clone)]
pub enum CaptureError {
    /// Camera permission is denied/restricted, or this process can't request it
    /// (e.g. an unbundled binary with no `NSCameraUsageDescription`).
    AccessDenied,
    /// No camera matched the requested unique id.
    NotFound,
    /// The session ran but produced no frame within the timeout.
    Timeout,
    /// An AVFoundation object failed to construct.
    Setup(String),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AccessDenied => write!(
                f,
                "camera access denied — grant Camera permission (and run inside an app bundle with NSCameraUsageDescription)"
            ),
            Self::NotFound => write!(f, "no camera matched that id"),
            Self::Timeout => write!(f, "camera produced no frame in time"),
            Self::Setup(s) => write!(f, "capture setup failed: {s}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// kCVPixelFormatType_32BGRA ('BGRA').
const PIXEL_FORMAT_32BGRA: u32 = 0x4247_5241;
/// kCVPixelBufferLock_ReadOnly.
const LOCK_READ_ONLY: u64 = 1;

// The most recent frame the delegate decoded. A process previews one camera at
// a time, so a single global sink is enough and keeps the delegate stateless.
static LATEST: OnceLock<Mutex<Option<Frame>>> = OnceLock::new();
fn latest() -> &'static Mutex<Option<Frame>> {
    LATEST.get_or_init(|| Mutex::new(None))
}

/// Increments on every delivered frame, so a poller can tell a new frame from a
/// repeat without comparing pixel buffers.
static FRAME_GEN: AtomicU64 = AtomicU64::new(0);

/// Target max width for delegate downsampling (0 = full resolution). Previews
/// set this so the delegate decimates the (possibly 1080p) buffer down to a
/// cheap thumbnail in one strided pass — far less copy + a tiny GPU texture.
static PREVIEW_TARGET_W: AtomicU32 = AtomicU32::new(0);

/// Counts delivered buffers so the delegate can drop most of them: the camera
/// pushes ~30 fps but the preview only needs ~10, and skipping before the
/// pixel-buffer lock makes the dropped frames almost free.
static FRAME_SKIP: AtomicU64 = AtomicU64::new(0);
/// Process one in every `FRAME_STRIDE` delivered frames (30 fps / 2 = 15 fps).
const FRAME_STRIDE: u64 = 2;

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeVideo: *const Object;
    static AVCaptureSessionPresetMedium: *const Object;
}

#[link(name = "CoreMedia", kind = "framework")]
unsafe extern "C" {
    fn CMSampleBufferGetImageBuffer(sbuf: *mut Object) -> *mut Object;
}

#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    static kCVPixelBufferPixelFormatTypeKey: *const Object;
    fn CVPixelBufferLockBaseAddress(pb: *mut Object, flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(pb: *mut Object, flags: u64) -> i32;
    fn CVPixelBufferGetBaseAddress(pb: *mut Object) -> *mut c_void;
    fn CVPixelBufferGetBytesPerRow(pb: *mut Object) -> usize;
    fn CVPixelBufferGetWidth(pb: *mut Object) -> usize;
    fn CVPixelBufferGetHeight(pb: *mut Object) -> usize;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopDefaultMode: *const c_void;
    fn CFRunLoopRunInMode(
        mode: *const c_void,
        seconds: f64,
        return_after_source_handled: BOOL,
    ) -> i32;
}

unsafe extern "C" {
    fn dispatch_queue_create(label: *const i8, attr: *const c_void) -> *mut Object;
}

/// Delegate callback: `captureOutput:didOutputSampleBuffer:fromConnection:`.
/// Converts the sample buffer's BGRA pixels to RGBA and stores them in [`latest`].
extern "C" fn did_output(
    _this: &Object,
    _sel: Sel,
    _output: *mut Object,
    sbuf: *mut Object,
    _conn: *mut Object,
) {
    // Throttle to ~10 fps before doing any work (the camera delivers ~30).
    if FRAME_SKIP.fetch_add(1, Ordering::Relaxed) % FRAME_STRIDE != 0 {
        return;
    }
    // SAFETY: `sbuf` is a valid CMSampleBuffer delivered by AVFoundation; the
    // image buffer is locked for the span of the read and unlocked before return.
    unsafe {
        let pb = CMSampleBufferGetImageBuffer(sbuf);
        if pb.is_null() || CVPixelBufferLockBaseAddress(pb, LOCK_READ_ONLY) != 0 {
            return;
        }
        let base = CVPixelBufferGetBaseAddress(pb).cast::<u8>();
        let bytes_per_row = CVPixelBufferGetBytesPerRow(pb);
        let width = CVPixelBufferGetWidth(pb);
        let height = CVPixelBufferGetHeight(pb);
        let target = PREVIEW_TARGET_W.load(Ordering::Relaxed) as usize;
        let step = if target > 0 && width > target {
            width.div_ceil(target)
        } else {
            1
        };
        let out_w = width / step;
        let out_h = height / step;
        if !base.is_null() && out_w > 0 && out_h > 0 {
            let mut rgba = vec![0u8; out_w * out_h * 4];
            for oy in 0..out_h {
                let row = base.add(oy * step * bytes_per_row);
                for ox in 0..out_w {
                    let src = row.add(ox * step * 4);
                    let out = (oy * out_w + ox) * 4;
                    rgba[out] = *src.add(2); // R <- B-G-R-A
                    rgba[out + 1] = *src.add(1);
                    rgba[out + 2] = *src;
                    rgba[out + 3] = *src.add(3);
                }
            }
            if let Ok(mut slot) = latest().lock() {
                *slot = Some(Frame {
                    width: out_w as u32,
                    height: out_h as u32,
                    rgba,
                });
                FRAME_GEN.fetch_add(1, Ordering::Relaxed);
            }
        }
        CVPixelBufferUnlockBaseAddress(pb, LOCK_READ_ONLY);
    }
}

fn delegate_class() -> *const Class {
    static CACHE: OnceLock<usize> = OnceLock::new();
    let ptr = *CACHE.get_or_init(|| {
        let superclass = class!(NSObject);
        match ClassDecl::new("OLCameraFrameDelegate", superclass) {
            Some(mut decl) => {
                // SAFETY: the registered selector matches `did_output`'s ABI
                // (the standard sample-buffer delegate signature).
                unsafe {
                    decl.add_method(
                        sel!(captureOutput:didOutputSampleBuffer:fromConnection:),
                        did_output
                            as extern "C" fn(&Object, Sel, *mut Object, *mut Object, *mut Object),
                    );
                }
                std::ptr::from_ref::<Class>(decl.register()) as usize
            }
            // Already registered (re-entry): look it up.
            None => Class::get("OLCameraFrameDelegate")
                .map_or(std::ptr::null::<Class>() as usize, |c| {
                    std::ptr::from_ref(c) as usize
                }),
        }
    });
    ptr as *const Class
}

/// Current Camera authorization: `Some(true)` usable, `Some(false)` denied,
/// `None` undetermined (caller should request access).
fn authorization() -> Option<bool> {
    let cls = class!(AVCaptureDevice);
    // SAFETY: documented class method returning an AVAuthorizationStatus NSInteger.
    let status: isize =
        unsafe { msg_send![cls, authorizationStatusForMediaType: AVMediaTypeVideo] };
    match status {
        3 => Some(true),
        1 | 2 => Some(false),
        _ => None,
    }
}

/// Request Camera access and block until the user answers (or `timeout`).
fn request_access(timeout: Duration) -> bool {
    let answered = std::sync::Arc::new(Mutex::new(None::<bool>));
    let sink = answered.clone();
    let handler = ConcreteBlock::new(move |granted: BOOL| {
        if let Ok(mut slot) = sink.lock() {
            *slot = Some(granted != NO);
        }
    });
    let handler = handler.copy();
    let cls = class!(AVCaptureDevice);
    // SAFETY: documented async class method taking an AVMediaType + a
    // `void(^)(BOOL)` completion block; the block outlives the call below.
    unsafe {
        let _: () = msg_send![cls, requestAccessForMediaType: AVMediaTypeVideo completionHandler: &*handler];
    }
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(slot) = answered.lock() {
            if let Some(granted) = *slot {
                return granted;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        run_loop_tick(0.05);
    }
}

/// Ensure the process may use the camera, requesting access if undetermined.
fn ensure_access() -> Result<(), CaptureError> {
    match authorization() {
        Some(true) => Ok(()),
        None if request_access(Duration::from_secs(30)) => Ok(()),
        _ => Err(CaptureError::AccessDenied),
    }
}

/// Whether the process currently holds Camera permission, without prompting.
/// Lets the GUI start a preview only when access is already granted (so it never
/// blocks the UI thread on the permission dialog).
#[must_use]
pub fn camera_access_granted() -> bool {
    matches!(authorization(), Some(true))
}

/// Pump the current thread's run loop briefly so AVFoundation callbacks fire.
fn run_loop_tick(seconds: f64) {
    // SAFETY: `kCFRunLoopDefaultMode` is a valid mode constant; the call returns
    // after `seconds` or the first handled source.
    unsafe {
        CFRunLoopRunInMode(kCFRunLoopDefaultMode, seconds, NO);
    }
}

fn device_with_unique_id(unique_id: &str) -> Option<StrongPtr> {
    let cls = class!(AVCaptureDevice);
    let Ok(ns) = CString::new(unique_id) else {
        return None;
    };
    // SAFETY: building an autoreleased NSString from a valid C string, then a
    // `deviceWithUniqueID:` lookup; the result is retained into a StrongPtr.
    unsafe {
        let nsstr: *mut Object = msg_send![class!(NSString), stringWithUTF8String: ns.as_ptr()];
        let device: *mut Object = msg_send![cls, deviceWithUniqueID: nsstr];
        if device.is_null() {
            None
        } else {
            Some(StrongPtr::retain(device))
        }
    }
}

/// A running capture session. Frames flow to the delegate on a background
/// dispatch queue and land in [`latest`]; dropping the session stops it.
struct Session {
    handle: StrongPtr,
    _output: StrongPtr,
    _delegate: StrongPtr,
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: `self.session` is a valid, retained AVCaptureSession.
        unsafe {
            let _: () = msg_send![*self.handle, stopRunning];
        }
    }
}

/// Authorize, wire up, and start a capture session on `unique_id`. Frames begin
/// arriving in [`latest`] shortly after this returns.
fn open_session(unique_id: &str, low_res: bool) -> Result<Session, CaptureError> {
    ensure_access()?;
    let device = device_with_unique_id(unique_id).ok_or(CaptureError::NotFound)?;
    if let Ok(mut slot) = latest().lock() {
        *slot = None;
    }
    // Previews decimate to a ~480px-wide thumbnail; snapshots keep full res.
    PREVIEW_TARGET_W.store(if low_res { 480 } else { 0 }, Ordering::Relaxed);

    // SAFETY: standard AVCaptureSession wiring with documented selectors; every
    // object added is retained by the session, and the session is stopped on Drop.
    unsafe {
        let session: *mut Object = msg_send![class!(AVCaptureSession), new];
        if session.is_null() {
            return Err(CaptureError::Setup("AVCaptureSession".into()));
        }
        let session = StrongPtr::new(session);

        let mut err: *mut Object = std::ptr::null_mut();
        let input: *mut Object = msg_send![
            class!(AVCaptureDeviceInput),
            deviceInputWithDevice: *device error: std::ptr::addr_of_mut!(err)
        ];
        if input.is_null() {
            return Err(CaptureError::Setup("AVCaptureDeviceInput".into()));
        }
        let can_in: BOOL = msg_send![*session, canAddInput: input];
        if can_in == NO {
            return Err(CaptureError::Setup("session rejected input".into()));
        }
        let _: () = msg_send![*session, addInput: input];

        // Preview streams capture at a reduced resolution — far less per-frame
        // copy + texture upload than native 1080p, which keeps the UI smooth.
        if low_res {
            let can: BOOL = msg_send![*session, canSetSessionPreset: AVCaptureSessionPresetMedium];
            if can != NO {
                let _: () = msg_send![*session, setSessionPreset: AVCaptureSessionPresetMedium];
            }
        }

        let output: *mut Object = msg_send![class!(AVCaptureVideoDataOutput), new];
        let output = StrongPtr::new(output);
        let num: *mut Object =
            msg_send![class!(NSNumber), numberWithUnsignedInt: PIXEL_FORMAT_32BGRA];
        let settings: *mut Object = msg_send![
            class!(NSDictionary),
            dictionaryWithObject: num forKey: kCVPixelBufferPixelFormatTypeKey
        ];
        let _: () = msg_send![*output, setVideoSettings: settings];
        let _: () = msg_send![*output, setAlwaysDiscardsLateVideoFrames: true];

        let delegate_cls = delegate_class();
        if delegate_cls.is_null() {
            return Err(CaptureError::Setup("delegate class".into()));
        }
        let cls_ref: &Class = &*delegate_cls;
        let delegate: *mut Object = msg_send![cls_ref, new];
        let delegate = StrongPtr::new(delegate);
        let queue = dispatch_queue_create(c"org.openlogi.camera".as_ptr(), std::ptr::null());
        let _: () = msg_send![*output, setSampleBufferDelegate: *delegate queue: queue];

        let can_out: BOOL = msg_send![*session, canAddOutput: *output];
        if can_out == NO {
            return Err(CaptureError::Setup("session rejected output".into()));
        }
        let _: () = msg_send![*session, addOutput: *output];
        let _: () = msg_send![*session, startRunning];

        Ok(Session {
            handle: session,
            _output: output,
            _delegate: delegate,
        })
    }
}

/// Capture a single RGBA frame from the camera with `unique_id`.
///
/// # Errors
/// [`CaptureError::AccessDenied`] when Camera permission isn't (and can't be)
/// granted, [`CaptureError::NotFound`] for an unknown id, [`CaptureError::Timeout`]
/// when no frame arrives, or [`CaptureError::Setup`] on AVFoundation errors.
pub fn capture_frame(unique_id: &str, timeout: Duration) -> Result<Frame, CaptureError> {
    let _session = open_session(unique_id, false)?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(mut slot) = latest().lock() {
            if let Some(frame) = slot.take() {
                return Ok(frame);
            }
        }
        if Instant::now() >= deadline {
            return Err(CaptureError::Timeout);
        }
        run_loop_tick(0.03);
    }
}

/// A live preview stream. Holds the session open; [`CameraStream::latest_frame`]
/// returns the most recent frame each time it's polled. Dropping it stops the
/// camera.
pub struct CameraStream {
    _session: Session,
}

impl CameraStream {
    /// The most recently delivered frame, or `None` before the first arrives.
    #[must_use]
    pub fn latest_frame(&self) -> Option<Frame> {
        latest().lock().ok().and_then(|slot| slot.clone())
    }

    /// A counter that increments on every delivered frame, so the preview can
    /// skip rebuilding its texture when no new frame has arrived.
    #[must_use]
    pub fn frame_generation(&self) -> u64 {
        FRAME_GEN.load(Ordering::Relaxed)
    }
}

/// Start a live capture stream on the camera with `unique_id`.
///
/// # Errors
/// Same as [`capture_frame`], minus `Timeout` (frames are polled, not awaited).
pub fn start_stream(unique_id: &str) -> Result<CameraStream, CaptureError> {
    Ok(CameraStream {
        _session: open_session(unique_id, true)?,
    })
}
