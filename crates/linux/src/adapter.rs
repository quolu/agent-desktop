use agent_desktop_core::{ActionOps, InputOps};
#[cfg(not(target_os = "linux"))]
use agent_desktop_core::{ObservationOps, SystemOps};

/// The Linux adapter: AT-SPI2 over D-Bus.
///
/// Implements the read path (`list-apps`, `list-windows`, `snapshot` of a
/// window). Actions, input, capture and everything else still return
/// `PLATFORM_NOT_SUPPORTED`. The accessibility-bus connection is opened on
/// first use and shared by every later read through this instance.
pub struct LinuxAdapter {
    #[cfg(target_os = "linux")]
    bus: std::sync::OnceLock<crate::system::a11y_bus::A11yBus>,
}

impl LinuxAdapter {
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            bus: std::sync::OnceLock::new(),
        }
    }
}

impl Default for LinuxAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use agent_desktop_core::{
        AccessibilityNode, AdapterError, AppInfo, Deadline, ObservationOps, ObservationRequest,
        ObservationRoot, ObservedTree, PermissionReport, PermissionState, SnapshotSurface,
        SystemOps, TreeOptions, WindowFilter, WindowInfo,
    };

    use super::LinuxAdapter;
    use crate::system::a11y_bus::{self, A11yBus};
    use crate::system::windows;
    use crate::tree::{deadline, observe};

    impl LinuxAdapter {
        fn bus(&self, deadline: Deadline) -> Result<&A11yBus, AdapterError> {
            if let Some(bus) = self.bus.get() {
                return Ok(bus);
            }
            let bus = a11y_bus::connect(deadline)?;
            Ok(self.bus.get_or_init(|| bus))
        }
    }

    impl ObservationOps for LinuxAdapter {
        fn observe_tree(
            &self,
            root: ObservationRoot<'_>,
            request: &ObservationRequest,
        ) -> Result<ObservedTree, AdapterError> {
            observe::observe_tree(self.bus(request.deadline)?, root, request)
        }

        /// The FFI legacy entrypoint over the same observation `snapshot` uses.
        fn get_tree(
            &self,
            window: &WindowInfo,
            options: &TreeOptions,
            deadline: Deadline,
        ) -> Result<AccessibilityNode, AdapterError> {
            self.observe_tree(
                ObservationRoot::Window(window),
                &ObservationRequest::snapshot(options, deadline),
            )?
            .into_accessibility_tree()
        }

        fn list_windows(
            &self,
            filter: &WindowFilter,
            deadline: Deadline,
        ) -> Result<Vec<WindowInfo>, AdapterError> {
            let connection = self.bus(deadline)?.connection();
            let records = zbus::block_on(deadline::within(
                deadline,
                windows::window_records(connection, filter),
            ))?;
            Ok(records.into_iter().map(|record| record.info).collect())
        }

        fn list_apps(&self, deadline: Deadline) -> Result<Vec<AppInfo>, AdapterError> {
            let connection = self.bus(deadline)?.connection();
            zbus::block_on(deadline::within(deadline, windows::app_list(connection)))
        }
    }

    impl SystemOps for LinuxAdapter {
        /// AT-SPI2 has no per-application permission: a reachable bus is the
        /// grant. An unreachable bus is reported as unknown so the command
        /// itself fails with `PLATFORM_NOT_SUPPORTED` and the instructions to
        /// enable AT-SPI2, rather than a misleading `PERM_DENIED`.
        fn permission_report(&self, deadline: Deadline) -> Result<PermissionReport, AdapterError> {
            let accessibility = match self.bus(deadline) {
                Ok(_) => PermissionState::Granted,
                Err(_) => PermissionState::Unknown,
            };
            Ok(PermissionReport {
                accessibility,
                screen_recording: PermissionState::Unknown,
                automation: PermissionState::NotRequired,
            })
        }

        fn supported_surfaces(&self) -> Vec<SnapshotSurface> {
            vec![SnapshotSurface::Window]
        }

        /// AT-SPI2 `ACTIVE` is per application: each toolkit marks the window
        /// it last activated, and on Wayland several applications claim it at
        /// once. A focused window is reported only when exactly one claims it.
        fn focused_window(&self, deadline: Deadline) -> Result<Option<WindowInfo>, AdapterError> {
            let filter = WindowFilter {
                focused_only: true,
                app: None,
            };
            let mut focused = self.list_windows(&filter, deadline)?;
            if focused.len() == 1 {
                Ok(focused.pop())
            } else {
                Ok(None)
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
impl ObservationOps for LinuxAdapter {}
#[cfg(not(target_os = "linux"))]
impl SystemOps for LinuxAdapter {}
impl ActionOps for LinuxAdapter {}
impl InputOps for LinuxAdapter {}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use agent_desktop_core::{SnapshotSurface, SystemOps};

    #[test]
    fn only_the_window_surface_is_offered() {
        assert_eq!(
            LinuxAdapter::new().supported_surfaces(),
            [SnapshotSurface::Window]
        );
    }
}
