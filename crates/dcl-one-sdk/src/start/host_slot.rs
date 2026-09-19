//! The preview's hold on the first project's authoritative-server isolate.
//!
//! scene.json's `authoritativeMultiplayer` can flip under a running preview
//! (the /scene page's switch, a hand edit), so the isolate lives in a slot on
//! [`AppState`] that follows the flag, not in a local of `start`. The host
//! joins the built-in ws-room and has no LiveKit transport, so while the scene
//! has a server the preview keeps comms there: clients on a LiveKit scene room
//! would never meet it.

use super::AppState;
use crate::live_reload::ReloadEvent;
use crate::ux;
use std::path::Path;
use std::sync::PoisonError;

/// Said once at startup, in place of spawning the embedded livekit-server.
pub(super) fn note_voice_off() {
    ux::note(
        "voice off for this preview: the authoritative host joins the built-in ws-room, \
         so comms stay there (--no-host brings voice back, without the server)",
    );
}

impl AppState {
    /// Whether the first project runs with a server: its scene.json flag as
    /// it reads now, unless `--no-host` took the server away.
    pub(super) fn hosts_scene(&self) -> bool {
        !self.no_host
            && self
                .first_project()
                .is_some_and(|p| crate::entrypoint::authoritative_multiplayer(&p))
    }

    /// The LiveKit server clients are sent to. None while the scene has a
    /// server, whatever `livekit` holds.
    pub(super) fn voice(&self) -> Option<&crate::livekit::Livekit> {
        self.livekit.as_ref().filter(|_| !self.hosts_scene())
    }

    /// The router's clones of the state outlive `start`; the isolate must not.
    pub(super) fn release_host(&self) {
        *self.host.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Whether the isolate is attached and still alive, not merely spawned.
    pub(super) fn host_running(&self) -> bool {
        let mut slot = self.host.lock().unwrap_or_else(PoisonError::into_inner);
        slot.as_mut()
            .is_some_and(|isolate| matches!(isolate.child.try_wait(), Ok(None)))
    }

    /// A rebuild is when the flag's flip lands and when the server's bundle
    /// goes stale; a model save is neither.
    pub(super) fn follow_reload(&self, root: &Path, event: &ReloadEvent) {
        if matches!(event, ReloadEvent::Scene) {
            self.follow_host(Some(root));
        }
    }

    /// Bring the server isolate in line with the flag: attach it when the flag
    /// is on, drop it when it is off, and after `rebuilt` was built swap a
    /// running one for a fresh isolate, since it runs the bundle it loaded.
    pub(super) fn follow_host(&self, rebuilt: Option<&Path>) {
        let Some(first) = self.first_project() else {
            return;
        };
        let wanted = self.hosts_scene();
        let stale = rebuilt.is_some_and(|root| root == first.root);
        let mut slot = self.host.lock().unwrap_or_else(PoisonError::into_inner);
        let was_running = slot.is_some();
        if was_running && (!wanted || stale) {
            *slot = None;
        }
        if !wanted {
            if was_running {
                ux::note_clocked(
                    "\u{25c9} multiplayer: host detached (scene.json authoritativeMultiplayer is off)",
                );
            }
            return;
        }
        if slot.is_some() {
            return;
        }
        match crate::host::spawn_isolate(
            &first.root,
            &format!("http://127.0.0.1:{}", self.port),
            "room-1",
        ) {
            Ok(isolate) => {
                *slot = Some(isolate);
                ux::note_arrow(if was_running {
                    "authoritative host restarted on the new build"
                } else {
                    "authoritative host attached (scene.json authoritativeMultiplayer; --no-host or --no-server to skip)"
                });
                if self.livekit.is_some() && !was_running {
                    ux::note(
                        "comms moved to the built-in ws-room while the scene has a server, which has no \
                         LiveKit transport; clients already in the preview rejoin to meet it",
                    );
                }
            }
            Err(e) => ux::report_watch(&e),
        }
    }
}
