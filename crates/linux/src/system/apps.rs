use std::time::Duration;

use agent_desktop_core::{AdapterError, AppInfo, AppPresentation, ErrorCode, ProcessId};
use futures_lite::future::zip;
use futures_util::future::join_all;
use zbus::Connection;

use super::process;
use crate::tree::deadline;
use crate::tree::element::{self, ACCESSIBLE, APPLICATION, ElementRef};

/// How long one application may take to describe itself before a listing
/// moves on without it.
pub(crate) const APP_READ_BUDGET: Duration = Duration::from_millis(1_000);

/// Processes that draw the desktop shell itself rather than application
/// windows. Their accessible "windows" are the compositor stage, not
/// anything a user opened.
const SHELL_APPLICATIONS: &[&str] = &["gnome-shell"];

/// One application registered on the accessibility bus.
pub(crate) struct AppRecord {
    pub(crate) name: String,
    pub(crate) pid: u32,
    pub(crate) command: Option<String>,
    pub(crate) toolkit: Option<String>,
    pub(crate) top_levels: Vec<ElementRef>,
}

impl AppRecord {
    pub(crate) fn is_shell(&self) -> bool {
        SHELL_APPLICATIONS
            .iter()
            .any(|shell| self.name.eq_ignore_ascii_case(shell))
    }

    /// Whether `query` names this application by its accessible name or by
    /// its executable name.
    pub(crate) fn matches(&self, query: &str) -> bool {
        agent_desktop_core::app_name_matches(&self.name, query)
            || self
                .command
                .as_deref()
                .is_some_and(|command| agent_desktop_core::app_name_matches(command, query))
    }

    pub(crate) fn info(&self, presentation: Option<AppPresentation>) -> AppInfo {
        AppInfo {
            name: self.name.clone(),
            pid: ProcessId::new(self.pid),
            bundle_id: None,
            process_instance: process::process_instance(self.pid),
            presentation,
        }
    }
}

/// An application whose process is known from the bus daemon but which did
/// not answer within [`APP_READ_BUDGET`].
pub(crate) struct UnresponsiveApp {
    pub(crate) pid: u32,
    pub(crate) command: Option<String>,
}

impl UnresponsiveApp {
    pub(crate) fn label(&self) -> String {
        self.command
            .clone()
            .unwrap_or_else(|| format!("pid {}", self.pid))
    }

    pub(crate) fn matches(&self, query: &str) -> bool {
        self.command
            .as_deref()
            .is_some_and(|command| agent_desktop_core::app_name_matches(command, query))
    }

    pub(crate) fn info(&self) -> AppInfo {
        AppInfo {
            name: self.label(),
            pid: ProcessId::new(self.pid),
            bundle_id: None,
            process_instance: process::process_instance(self.pid),
            presentation: None,
        }
    }
}

/// Every application on the bus: those that described themselves, and those
/// that did not answer in time.
pub(crate) struct AppScan {
    pub(crate) apps: Vec<AppRecord>,
    pub(crate) unresponsive: Vec<UnresponsiveApp>,
}

enum AppRead {
    Ready(AppRecord),
    Unresponsive(UnresponsiveApp),
    Gone,
}

/// Reads every application the AT-SPI2 registry knows, concurrently. An
/// application that exits while being read is skipped, one that does not
/// answer is set aside as unresponsive, and only the registry itself failing
/// is an error.
pub(crate) async fn app_records(connection: &Connection) -> Result<AppScan, AdapterError> {
    let roots = element::children(connection, &ElementRef::registry_root())
        .await
        .map_err(registry_unreadable)?;
    let reads = roots
        .into_iter()
        .map(|root| read_app(connection, root.application_root()));
    let mut scan = AppScan {
        apps: Vec::new(),
        unresponsive: Vec::new(),
    };
    for read in join_all(reads).await {
        match read {
            AppRead::Ready(app) => scan.apps.push(app),
            AppRead::Unresponsive(app) => scan.unresponsive.push(app),
            AppRead::Gone => {}
        }
    }
    Ok(scan)
}

async fn read_app(connection: &Connection, root: ElementRef) -> AppRead {
    let Ok(pid) = element::process_id(connection, &root.bus).await else {
        return AppRead::Gone;
    };
    let reads = zip(
        zip(
            element::string_property(connection, &root, ACCESSIBLE, "Name"),
            element::string_property(connection, &root, APPLICATION, "ToolkitName"),
        ),
        element::children(connection, &root),
    );
    let Some(((name, toolkit), top_levels)) = deadline::bounded(APP_READ_BUDGET, reads).await
    else {
        return AppRead::Unresponsive(UnresponsiveApp {
            pid,
            command: process::command_name(pid),
        });
    };
    match describe(pid, name, toolkit, top_levels) {
        Some(app) => AppRead::Ready(app),
        None => AppRead::Gone,
    }
}

fn describe(
    pid: u32,
    name: zbus::Result<String>,
    toolkit: zbus::Result<String>,
    top_levels: zbus::Result<Vec<ElementRef>>,
) -> Option<AppRecord> {
    let name = name
        .ok()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| process::command_name(pid))?;
    Some(AppRecord {
        name,
        pid,
        command: process::command_name(pid),
        toolkit: toolkit.ok(),
        top_levels: top_levels.unwrap_or_default(),
    })
}

fn registry_unreadable(error: zbus::Error) -> AdapterError {
    AdapterError::new(
        ErrorCode::PlatformNotSupported,
        "The AT-SPI2 registry did not list its applications",
    )
    .with_suggestion("Check that at-spi2-registryd is running in the graphical session")
    .with_platform_detail(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, command: Option<&str>) -> AppRecord {
        AppRecord {
            name: name.to_string(),
            pid: 1,
            command: command.map(str::to_string),
            toolkit: None,
            top_levels: Vec::new(),
        }
    }

    #[test]
    fn an_app_matches_its_accessible_or_executable_name() {
        let chrome = record("Google Chrome", Some("chrome"));
        assert!(chrome.matches("google chrome"));
        assert!(chrome.matches("Chrome"));
        assert!(!chrome.matches("chromium"));
    }

    #[test]
    fn an_unresponsive_app_is_named_by_its_executable() {
        let hung = UnresponsiveApp {
            pid: 7,
            command: Some("chrome".to_string()),
        };
        assert!(hung.matches("Chrome"));
        assert_eq!(hung.info().name, "chrome");
        assert_eq!(hung.info().presentation, None);
        let nameless = UnresponsiveApp {
            pid: 7,
            command: None,
        };
        assert_eq!(nameless.label(), "pid 7");
        assert!(!nameless.matches("chrome"));
    }

    #[test]
    fn the_desktop_shell_is_recognized() {
        assert!(record("gnome-shell", None).is_shell());
        assert!(!record("Files", Some("nautilus")).is_shell());
    }
}
