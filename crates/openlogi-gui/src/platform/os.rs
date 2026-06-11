//! Best-effort host OS version string for the diagnostics report.

/// The OS product version (e.g. `"15.5"` on macOS), or `None` when unavailable.
#[must_use]
pub fn os_version() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let version = String::from_utf8(out.stdout).ok()?.trim().to_string();
        if version.is_empty() {
            None
        } else {
            Some(version)
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
