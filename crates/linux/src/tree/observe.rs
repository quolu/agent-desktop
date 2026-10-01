use agent_desktop_core::{
    AdapterError, ErrorCode, ObservationRequest, ObservationRoot, ObservationSource, ObservedTree,
    SnapshotSurface,
};
use serde_json::json;

use super::deadline;
use super::walk::{self, WalkRequest};
use crate::system::a11y_bus::A11yBus;
use crate::system::windows;

/// Observes one window's accessibility tree.
///
/// Only the window surface rooted at a listed window is supported so far;
/// drill-down from a stored ref needs element re-identification, which this
/// adapter does not implement yet, and says so rather than guessing.
pub(crate) fn observe_tree(
    bus: &A11yBus,
    root: ObservationRoot<'_>,
    request: &ObservationRequest,
) -> Result<ObservedTree, AdapterError> {
    let request = (*request).validate()?;
    let ObservationRoot::Window(window) = root else {
        return Err(AdapterError::not_supported(
            "observe_tree from an element ref",
        ));
    };
    if request.surface != SnapshotSurface::Window {
        return Err(AdapterError::not_supported(
            "observe_tree for a non-window surface",
        ));
    }
    let connection = bus.connection();
    zbus::block_on(async {
        let record =
            deadline::within(request.deadline, windows::find_window(connection, window)).await?;
        let outcome = walk::walk(
            connection,
            record.element,
            WalkRequest {
                max_logical_depth: request.max_logical_depth,
                max_raw_depth: request.max_raw_depth,
                budget: request.budget,
                deadline: request.deadline,
                bounds_trusted: record.bounds_trusted,
            },
        )
        .await;
        if outcome.root_defunct {
            return Err(AdapterError::new(
                ErrorCode::WindowNotFound,
                format!("Window {} closed while it was being observed", window.id),
            )
            .with_suggestion("Run 'list-windows' to see the windows that are open now."));
        }
        let root_tree = outcome.root.ok_or_else(|| {
            AdapterError::new(
                ErrorCode::Timeout,
                "Accessibility observation ended before reading its root",
            )
            .with_details(json!({
                "kind": "observation_root_incomplete",
                "complete": false,
                "query_stats": outcome.stats,
            }))
        })?;
        let complete = root_tree.is_complete();
        ObservedTree::from_roots(
            vec![root_tree],
            ObservationSource::from_root(&root, SnapshotSurface::Window),
            outcome.stats,
            complete,
        )
    })
}
