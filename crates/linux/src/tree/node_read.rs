use agent_desktop_core::{
    IdentifierEvidence, LocatorEvidence, LocatorField, LocatorRefEvidence, Rect,
};
use futures_lite::future::zip;
use futures_util::future::join_all;
use zbus::Connection;

use super::actions::{AffordanceEvidence, available_actions};
use super::element::{
    self, ACCESSIBLE, ACTION, COMPONENT, COORD_SCREEN, EDITABLE_TEXT, ElementRef, VALUE,
};
use super::roles;
use super::states::AtspiStates;
use super::value_read;

const MAX_ACTIONS: i32 = 32;

/// The first round of reads for one element: everything the walker needs to
/// decide whether to descend into it. Each slot is `None` when its read
/// failed, so a failure is never mistaken for an empty answer.
pub(crate) struct NodeBasics {
    pub(crate) role_code: Option<u32>,
    pub(crate) states: Option<AtspiStates>,
    pub(crate) text: AccessibleText,
    pub(crate) interfaces: Option<Vec<String>>,
}

/// The `Accessible` properties the walk reads, each `None` when its read
/// failed.
pub(crate) struct AccessibleText {
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) child_count: Option<i32>,
}

/// What the second round of reads should fetch.
#[derive(Clone, Copy)]
pub(crate) struct DetailPlan {
    pub(crate) bounds_trusted: bool,
    pub(crate) children: bool,
    pub(crate) max_field_bytes: usize,
}

/// The second round of reads, issued once the walker knows the element is
/// part of the observation.
pub(crate) struct NodeDetails {
    pub(crate) bounds: LocatorField<Rect>,
    pub(crate) action_names: Option<Vec<String>>,
    pub(crate) value: LocatorField<String>,
    pub(crate) children: Option<Vec<ElementRef>>,
}

impl NodeBasics {
    /// Every read failed: the object is gone (its application exited or the
    /// toolkit destroyed it mid-walk), not merely uncooperative.
    pub(crate) fn is_defunct(&self) -> bool {
        self.role_code.is_none()
            && self.states.is_none()
            && self.text.name.is_none()
            && self.interfaces.is_none()
    }

    pub(crate) fn name(&self) -> LocatorField<String> {
        known_or_unknown(&self.text.name)
    }

    pub(crate) fn child_count(&self) -> Option<u32> {
        self.text
            .child_count
            .and_then(|count| u32::try_from(count).ok())
    }

    pub(crate) fn has_interface(&self, interface: &str) -> bool {
        self.interfaces
            .as_ref()
            .is_some_and(|interfaces| interfaces.iter().any(|name| name == interface))
    }

    pub(crate) fn secure(&self) -> bool {
        self.role_code.is_some_and(roles::is_password_role)
    }

    /// A nameless panel, section or filler that does not consume depth.
    pub(crate) fn is_web_wrapper(&self) -> bool {
        self.role_code.is_some_and(roles::is_wrapper_role)
            && self
                .name()
                .known()
                .is_none_or(|name| name.trim().is_empty())
            && !self.has_interface(VALUE)
            && !self.has_interface(EDITABLE_TEXT)
    }

    /// Projects both rounds onto the evidence shape core consumes.
    pub(crate) fn evidence(&self, details: &NodeDetails) -> LocatorEvidence {
        let role = match (self.role_code, self.states) {
            (Some(code), Some(states)) => {
                LocatorField::Known(roles::canonical_role(code, &states).as_str().to_string())
            }
            (Some(code), None) => LocatorField::Known(
                roles::canonical_role(code, &AtspiStates::from_words(&[]))
                    .as_str()
                    .to_string(),
            ),
            (None, _) => LocatorField::Unknown,
        };
        let states = self.states.map_or(LocatorField::Unknown, |states| {
            LocatorField::Known(states.canonical(self.secure()))
        });
        let available_actions = match (&details.action_names, self.states) {
            (Some(names), Some(states)) => {
                LocatorField::Known(available_actions(&AffordanceEvidence {
                    action_names: names,
                    states,
                    editable_text: self.has_interface(EDITABLE_TEXT),
                    settable_value: self.has_interface(VALUE)
                        && self.role_code.is_some_and(roles::is_adjustable_role),
                }))
            }
            _ => LocatorField::Unknown,
        };
        LocatorEvidence {
            role,
            name: self.name(),
            description: known_or_unknown(&self.text.description),
            value: details.value.clone(),
            identifiers: IdentifierEvidence::absent(),
            states,
            ref_evidence: LocatorRefEvidence {
                bounds: details.bounds.clone(),
                available_actions,
            },
        }
    }
}

fn known_or_unknown(value: &Option<String>) -> LocatorField<String> {
    value
        .clone()
        .map_or(LocatorField::Unknown, LocatorField::Known)
}

pub(crate) async fn read_basics(connection: &Connection, element: &ElementRef) -> NodeBasics {
    let ((role_code, states), (interfaces, text)) = zip(
        zip(
            element::call::<_, u32>(connection, element, ACCESSIBLE, "GetRole", &()),
            element::call::<_, Vec<u32>>(connection, element, ACCESSIBLE, "GetState", &()),
        ),
        zip(
            element::call::<_, Vec<String>>(connection, element, ACCESSIBLE, "GetInterfaces", &()),
            read_accessible_text(connection, element),
        ),
    )
    .await;
    NodeBasics {
        role_code: role_code.ok(),
        states: states.ok().map(|words| AtspiStates::from_words(&words)),
        text,
        interfaces: interfaces.ok(),
    }
}

async fn read_accessible_text(connection: &Connection, element: &ElementRef) -> AccessibleText {
    let ((name, description), child_count) = zip(
        zip(
            element::string_property(connection, element, ACCESSIBLE, "Name"),
            element::string_property(connection, element, ACCESSIBLE, "Description"),
        ),
        element::int_property(connection, element, ACCESSIBLE, "ChildCount"),
    )
    .await;
    AccessibleText {
        name: name.ok(),
        description: description.ok(),
        child_count: child_count.ok(),
    }
}

pub(crate) async fn read_details(
    connection: &Connection,
    element: &ElementRef,
    basics: &NodeBasics,
    plan: DetailPlan,
) -> NodeDetails {
    let ((bounds, action_names), (value, children)) = zip(
        zip(
            read_bounds(connection, element, basics, plan.bounds_trusted),
            read_action_names(connection, element, basics),
        ),
        zip(
            value_read::read_value(connection, element, basics, plan.max_field_bytes),
            read_children(connection, element, plan.children),
        ),
    )
    .await;
    NodeDetails {
        bounds,
        action_names,
        value,
        children,
    }
}

async fn read_bounds(
    connection: &Connection,
    element: &ElementRef,
    basics: &NodeBasics,
    trusted: bool,
) -> LocatorField<Rect> {
    if !trusted || !basics.has_interface(COMPONENT) {
        return LocatorField::Absent;
    }
    match element::call::<_, (i32, i32, i32, i32)>(
        connection,
        element,
        COMPONENT,
        "GetExtents",
        &COORD_SCREEN,
    )
    .await
    {
        Ok(extents) => extents_rect(extents).map_or(LocatorField::Absent, LocatorField::Known),
        Err(_) => LocatorField::Unknown,
    }
}

/// AT-SPI2 reports an element without geometry as `-1`/`0` extents; those
/// are absent bounds, not a rectangle at the origin.
pub(crate) fn extents_rect((x, y, width, height): (i32, i32, i32, i32)) -> Option<Rect> {
    if width <= 0 || height <= 0 || (x == -1 && y == -1) {
        return None;
    }
    Some(Rect {
        x: f64::from(x),
        y: f64::from(y),
        width: f64::from(width),
        height: f64::from(height),
    })
}

/// Reads the non-localized action names. `GetActions` is not used because it
/// returns display names (GTK answers `クリック` for `click`, Chromium an empty
/// string); `GetName` per index is the stable name libatspi reads too.
async fn read_action_names(
    connection: &Connection,
    element: &ElementRef,
    basics: &NodeBasics,
) -> Option<Vec<String>> {
    basics.interfaces.as_ref()?;
    if !basics.has_interface(ACTION) {
        return Some(Vec::new());
    }
    let count = element::int_property(connection, element, ACTION, "NActions")
        .await
        .ok()?
        .clamp(0, MAX_ACTIONS);
    let reads = (0..count).map(|index| async move {
        element::call::<_, String>(connection, element, ACTION, "GetName", &index).await
    });
    join_all(reads)
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .ok()
}

async fn read_children(
    connection: &Connection,
    element: &ElementRef,
    wanted: bool,
) -> Option<Vec<ElementRef>> {
    if !wanted {
        return Some(Vec::new());
    }
    element::children(connection, element).await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extents_without_area_are_absent() {
        assert!(extents_rect((-1, -1, -1, -1)).is_none());
        assert!(extents_rect((10, 10, 0, 20)).is_none());
        assert_eq!(
            extents_rect((166, 152, 1788, 1121)),
            Some(Rect {
                x: 166.0,
                y: 152.0,
                width: 1788.0,
                height: 1121.0
            })
        );
    }

    #[test]
    fn a_node_whose_every_read_failed_is_defunct() {
        let basics = NodeBasics {
            role_code: None,
            states: None,
            text: AccessibleText {
                name: None,
                description: None,
                child_count: None,
            },
            interfaces: None,
        };
        assert!(basics.is_defunct());
        assert!(basics.name().is_unknown());
    }

    #[test]
    fn a_nameless_panel_is_a_wrapper_but_a_named_one_is_not() {
        let panel = |name: &str| NodeBasics {
            role_code: Some(39),
            states: Some(AtspiStates::from_words(&[])),
            text: AccessibleText {
                name: Some(name.to_string()),
                description: Some(String::new()),
                child_count: Some(0),
            },
            interfaces: Some(vec![ACCESSIBLE.to_string()]),
        };
        assert!(panel("").is_web_wrapper());
        assert!(!panel("Sidebar").is_web_wrapper());
    }
}
