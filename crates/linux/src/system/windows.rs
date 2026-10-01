use agent_desktop_core::{
    AdapterError, AppPresentation, ErrorCode, ProcessId, Rect, WindowFilter, WindowInfo,
    WindowState,
};
use futures_lite::future::zip;
use futures_util::future::join_all;
use zbus::Connection;

use super::apps::{APP_READ_BUDGET, AppRecord, app_records};
use super::process;
use crate::tree::deadline;
use crate::tree::element::{self, COMPONENT, COORD_SCREEN, COORD_WINDOW, ElementRef};
use crate::tree::node_read::{self, NodeBasics, extents_rect};

const WINDOW_ROLES: &[u32] = &[2, 9, 14, 16, 19, 22, 23, 36, 69];
const COORDINATE_TRUSTED_TOOLKITS: &[&str] = &["chromium"];

/// A top-level window together with the accessible object that roots it.
pub(crate) struct WindowRecord {
    pub(crate) info: WindowInfo,
    pub(crate) element: ElementRef,
    pub(crate) bounds_trusted: bool,
}

/// Lists top-level windows of every application on the bus, filtered as the
/// caller asked. An application that does not answer is left out of a
/// general listing; when the caller named it and nothing else matched, the
/// listing fails with `APP_UNRESPONSIVE` instead of claiming it has no window.
pub(crate) async fn window_records(
    connection: &Connection,
    filter: &WindowFilter,
) -> Result<Vec<WindowRecord>, AdapterError> {
    let scan = app_records(connection).await?;
    let query = filter.app.as_deref();
    let wanted: Vec<&AppRecord> = scan
        .apps
        .iter()
        .filter(|app| !app.is_shell() && query.is_none_or(|query| app.matches(query)))
        .collect();
    let reads = wanted
        .iter()
        .map(|app| deadline::bounded(APP_READ_BUDGET, app_windows(connection, app)));
    let mut unresponsive: Vec<String> = query
        .map(|query| {
            scan.unresponsive
                .iter()
                .filter(|app| app.matches(query))
                .map(|app| app.label())
                .collect()
        })
        .unwrap_or_default();
    let mut windows = Vec::new();
    for (app, read) in wanted.iter().zip(join_all(reads).await) {
        match read {
            Some(app_windows) => windows.extend(app_windows),
            None => unresponsive.push(app.name.clone()),
        }
    }
    windows.retain(|window: &WindowRecord| !filter.focused_only || window.info.state.is_focused);
    if windows.is_empty() && query.is_some() {
        if let Some(app) = unresponsive.first() {
            return Err(AdapterError::app_unresponsive(app));
        }
    }
    Ok(windows)
}

/// Every application with how it presents itself: one that owns at least one
/// window is a foreground application.
pub(crate) async fn app_list(
    connection: &Connection,
) -> Result<Vec<agent_desktop_core::AppInfo>, AdapterError> {
    let scan = app_records(connection).await?;
    let reads = scan
        .apps
        .iter()
        .map(|app| deadline::bounded(APP_READ_BUDGET, app_windows(connection, app)));
    let windows = join_all(reads).await;
    let mut apps: Vec<agent_desktop_core::AppInfo> = scan
        .apps
        .iter()
        .zip(windows)
        .map(|(app, windows)| match windows {
            Some(windows) if !windows.is_empty() && !app.is_shell() => {
                app.info(Some(AppPresentation::Foreground))
            }
            Some(_) => app.info(Some(AppPresentation::Background)),
            None => app.info(None),
        })
        .collect();
    apps.extend(scan.unresponsive.iter().map(|app| app.info()));
    Ok(apps)
}

/// Finds the live window a previously listed `WindowInfo` describes, refusing
/// a window whose process has since been replaced.
pub(crate) async fn find_window(
    connection: &Connection,
    window: &WindowInfo,
) -> Result<WindowRecord, AdapterError> {
    let filter = WindowFilter {
        focused_only: false,
        app: None,
    };
    let records = window_records(connection, &filter).await?;
    records
        .into_iter()
        .find(|record| {
            record.info.id == window.id
                && record.info.pid == window.pid
                && (window.process_instance.is_none()
                    || record.info.process_instance == window.process_instance)
        })
        .ok_or_else(|| {
            AdapterError::new(
                ErrorCode::WindowNotFound,
                format!("Window {} is no longer open", window.id),
            )
            .with_suggestion("Run 'list-windows' to see the windows that are open now.")
        })
}

async fn app_windows(connection: &Connection, app: &AppRecord) -> Vec<WindowRecord> {
    let reads = app
        .top_levels
        .iter()
        .map(|top_level| read_window(connection, app, top_level));
    join_all(reads).await.into_iter().flatten().collect()
}

async fn read_window(
    connection: &Connection,
    app: &AppRecord,
    top_level: &ElementRef,
) -> Option<WindowRecord> {
    let (basics, (screen, relative)) = zip(
        node_read::read_basics(connection, top_level),
        zip(
            extents(connection, top_level, COORD_SCREEN),
            extents(connection, top_level, COORD_WINDOW),
        ),
    )
    .await;
    if !basics
        .role_code
        .is_some_and(|code| WINDOW_ROLES.contains(&code))
    {
        return None;
    }
    let bounds_trusted = coordinates_trusted(app.toolkit.as_deref(), screen, relative);
    let bounds = if bounds_trusted {
        screen.and_then(extents_rect)
    } else {
        None
    };
    Some(WindowRecord {
        info: window_info(app, top_level, &basics, bounds),
        element: top_level.clone(),
        bounds_trusted,
    })
}

async fn extents(
    connection: &Connection,
    element: &ElementRef,
    coordinates: u32,
) -> Option<(i32, i32, i32, i32)> {
    element::call(connection, element, COMPONENT, "GetExtents", &coordinates)
        .await
        .ok()
}

/// Screen coordinates are only trustworthy where the toolkit can know them.
/// A native Wayland client cannot learn its global position, so GTK and
/// clutter report window-relative extents in place of screen extents; the
/// two agreeing is that signature. Chromium (X11 or XWayland) always knows
/// its position, including when it sits at the screen origin.
fn coordinates_trusted(
    toolkit: Option<&str>,
    screen: Option<(i32, i32, i32, i32)>,
    relative: Option<(i32, i32, i32, i32)>,
) -> bool {
    let trusted_toolkit = toolkit.is_some_and(|toolkit| {
        COORDINATE_TRUSTED_TOOLKITS
            .iter()
            .any(|trusted| toolkit.eq_ignore_ascii_case(trusted))
    });
    match (screen, relative) {
        (Some(screen), Some(relative)) => trusted_toolkit || screen != relative,
        (Some(_), None) => trusted_toolkit,
        (None, _) => false,
    }
}

fn window_info(
    app: &AppRecord,
    top_level: &ElementRef,
    basics: &NodeBasics,
    bounds: Option<Rect>,
) -> WindowInfo {
    let states = basics.states;
    WindowInfo {
        id: window_id(app.pid, &top_level.path),
        title: basics.name().known().cloned().unwrap_or_default(),
        app: app.name.clone(),
        pid: ProcessId::new(app.pid),
        process_instance: process::process_instance(app.pid),
        bounds,
        state: WindowState {
            is_focused: states.is_some_and(|states| states.active()),
            accessible: true,
            minimized: states.map(|states| states.iconified()),
            visible: states.map(|states| states.showing() && !states.iconified()),
        },
    }
}

/// A window id stable for as long as the window's accessible object lives:
/// the owning pid plus a hash of the object path, which is unique within that
/// application.
pub(crate) fn window_id(pid: u32, path: &str) -> String {
    format!("w-{pid}-{:08x}", fnv1a(path))
}

fn fnv1a(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
