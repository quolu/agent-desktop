use agent_desktop_core::Role;
use agent_desktop_core::roles::INTERACTIVE_ROLES;

use super::*;
use crate::tree::states::AtspiStates;

const LAST_DEFINED_ROLE: u32 = 131;

fn plain() -> AtspiStates {
    AtspiStates::from_words(&[0, 0])
}

#[test]
fn every_defined_role_number_maps_to_a_canonical_role() {
    let unmapped: Vec<u32> = (1..LAST_DEFINED_ROLE)
        .filter(|code| !matches!(code, 67 | 70))
        .filter(|code| canonical_role(*code, &plain()) == Role::Unknown)
        .collect();
    assert!(unmapped.is_empty(), "unmapped AT-SPI roles: {unmapped:?}");
}

#[test]
fn unknown_and_future_roles_stay_unknown() {
    for code in [0, 67, 70, LAST_DEFINED_ROLE, 400] {
        assert_eq!(canonical_role(code, &plain()), Role::Unknown);
    }
}

#[test]
fn common_controls_map_to_interactive_roles() {
    for (code, expected) in [
        (43, "button"),
        (62, "button"),
        (7, "checkbox"),
        (44, "radiobutton"),
        (79, "textfield"),
        (40, "textfield"),
        (88, "link"),
        (35, "menuitem"),
        (37, "tab"),
        (91, "treeitem"),
        (11, "combobox"),
        (51, "slider"),
        (52, "incrementor"),
        (130, "switch"),
    ] {
        let role = canonical_role(code, &plain());
        assert_eq!(role.as_str(), expected, "role {code}");
        assert!(INTERACTIVE_ROLES.contains(&role.as_str()), "role {code}");
    }
}

#[test]
fn top_level_and_document_roles_are_distinguished() {
    assert_eq!(canonical_role(23, &plain()), Role::Window);
    assert_eq!(canonical_role(16, &plain()), Role::Dialog);
    assert_eq!(canonical_role(95, &plain()), Role::WebArea);
    assert_eq!(canonical_role(82, &plain()), Role::Document);
}

#[test]
fn a_list_item_is_an_option_only_when_selectable() {
    let selectable = AtspiStates::from_words(&[1 << 22, 0]);
    assert_eq!(canonical_role(32, &selectable), Role::Option);
    assert_eq!(canonical_role(32, &plain()), Role::Group);
}

#[test]
fn a_text_role_is_a_field_only_when_editable() {
    let editable = AtspiStates::from_words(&[1 << 7, 0]);
    assert_eq!(canonical_role(61, &editable), Role::TextField);
    assert_eq!(canonical_role(61, &plain()), Role::StaticText);
}

#[test]
fn wrappers_are_panels_sections_and_fillers() {
    assert!(is_wrapper_role(39));
    assert!(is_wrapper_role(85));
    assert!(is_wrapper_role(20));
    assert!(!is_wrapper_role(43));
}
