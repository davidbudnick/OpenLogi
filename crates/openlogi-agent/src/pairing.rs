//! Agent-side device pairing, exposed to the GUI over IPC.
//!
//! The agent owns all device I/O, so pairing — which opens the receiver — must
//! run here: a GUI that opened a receiver channel would clash with the agent's
//! live capture session on the same Bolt receiver (one process can't read the
//! same HID node through two channels). The GUI drives this over IPC
//! (`start_pairing` / `pair_device` / `cancel_pairing` + a `next_pairing`
//! long-poll for the event stream).
//!
//! While a session runs, the agent pauses its own capture via
//! [`SharedRuntime::pairing_active`] and waits for [`SharedRuntime::capture_idle`]
//! so `run_pairing` can own the receiver's HID node, then resumes capture when
//! the session ends (every end — including cancel — emits a terminal event).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use openlogi_agent_core::ipc::{FoundDevice, PairingUpdate};
use openlogi_agent_core::orchestrator::SharedRuntime;
use openlogi_agent_core::watchers::pairing::{self, Control};
use openlogi_hid::{DiscoveredDevice, PairingEvent, ReceiverSelector};
use tokio::sync::{Mutex, mpsc};
use tracing::warn;

/// How long the agent holds a `next_pairing` long-poll before returning `None`.
/// Comfortably under the client's request deadline so the agent answers first.
const HOLD: Duration = Duration::from_secs(20);

/// Address-keyed cache of the full discovered devices, so the GUI can pair by
/// address without round-tripping the non-serializable `DiscoveredDevice`.
type DeviceCache = Arc<StdMutex<HashMap<[u8; 6], DiscoveredDevice>>>;

/// Owns the pairing watcher and translates its event stream for the IPC layer.
pub struct PairingManager {
    ctrl: mpsc::UnboundedSender<Control>,
    updates: Mutex<mpsc::UnboundedReceiver<PairingUpdate>>,
    devices: DeviceCache,
    shared: SharedRuntime,
}

impl PairingManager {
    /// Spawn the pairing watcher and its event translator. One per agent; must
    /// be called inside the tokio runtime (it spawns the translator task).
    #[must_use]
    pub fn new(shared: SharedRuntime) -> Self {
        let (ctrl, raw_events) = pairing::spawn();
        let (upd_tx, upd_rx) = mpsc::unbounded_channel();
        let devices: DeviceCache = Arc::new(StdMutex::new(HashMap::new()));
        tokio::spawn(translate(
            raw_events,
            upd_tx,
            Arc::clone(&devices),
            Arc::clone(&shared.pairing_active),
        ));
        Self {
            ctrl,
            updates: Mutex::new(upd_rx),
            devices,
            shared,
        }
    }

    /// Begin a session: forget the previous discovery, pause capture, then start.
    pub async fn start(&self, selector: ReceiverSelector) {
        if let Ok(mut devices) = self.devices.lock() {
            devices.clear();
        }
        self.shared.pairing_active.store(true, Ordering::Relaxed);
        self.wait_capture_idle().await;
        let _ = self.ctrl.send(Control::Start(selector));
    }

    /// Pair with a previously discovered device by address.
    pub fn pair(&self, address: [u8; 6]) {
        let device = self
            .devices
            .lock()
            .ok()
            .and_then(|devices| devices.get(&address).cloned());
        if let Some(device) = device {
            let _ = self.ctrl.send(Control::Pair(device));
        } else {
            warn!(?address, "pair requested for an unknown device");
        }
    }

    /// Cancel the in-progress session. The resulting `Failed(Cancelled)` event
    /// resumes capture via the translator — don't clear `pairing_active` here, or
    /// capture could re-acquire the receiver while `run_pairing` still holds it.
    pub fn cancel(&self) {
        let _ = self.ctrl.send(Control::Cancel);
    }

    /// Long-poll the next pairing step; `None` when the hold window elapses.
    pub async fn next_update(&self) -> Option<PairingUpdate> {
        let mut rx = self.updates.lock().await;
        tokio::time::timeout(HOLD, rx.recv()).await.ok().flatten()
    }

    /// Wait (bounded, ~3s at the watcher's ~1s re-evaluation cadence) for the
    /// capture watcher to drop its session before opening the receiver.
    async fn wait_capture_idle(&self) {
        for _ in 0..30 {
            if self.shared.capture_idle.load(Ordering::Relaxed) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        warn!("capture did not release before pairing — proceeding anyway");
    }
}

/// Translate raw [`PairingEvent`]s into wire [`PairingUpdate`]s: cache each
/// discovered device by address (so `pair_device` can look it up), and resume
/// the agent's capture on every terminal event.
async fn translate(
    mut raw: mpsc::UnboundedReceiver<PairingEvent>,
    upd_tx: mpsc::UnboundedSender<PairingUpdate>,
    devices: DeviceCache,
    pairing_active: Arc<AtomicBool>,
) {
    while let Some(event) = raw.recv().await {
        let update = match event {
            PairingEvent::Searching => PairingUpdate::Searching,
            PairingEvent::DeviceFound(device) => {
                let found = FoundDevice {
                    address: device.address,
                    name: device.name.clone(),
                };
                if let Ok(mut devices) = devices.lock() {
                    devices.insert(device.address, device);
                }
                PairingUpdate::DeviceFound(found)
            }
            PairingEvent::Passkey(method) => PairingUpdate::Passkey(method),
            PairingEvent::Paired { slot } => PairingUpdate::Paired { slot },
            PairingEvent::Failed(error) => PairingUpdate::Failed(error.to_string()),
        };
        if matches!(
            update,
            PairingUpdate::Paired { .. } | PairingUpdate::Failed(_)
        ) {
            pairing_active.store(false, Ordering::Relaxed);
        }
        if upd_tx.send(update).is_err() {
            return; // the manager (and its receiver) is gone
        }
    }
}
