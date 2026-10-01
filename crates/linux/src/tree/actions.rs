use agent_desktop_core::capability;

use super::states::AtspiStates;

/// What the walker learned about an element's affordances: the raw
/// `Action.GetActions` names plus the interfaces and states that imply
/// actions AT-SPI2 does not list as named actions.
pub(crate) struct AffordanceEvidence<'a> {
    pub(crate) action_names: &'a [String],
    pub(crate) states: AtspiStates,
    pub(crate) editable_text: bool,
    pub(crate) settable_value: bool,
}

/// Projects AT-SPI2 affordances onto the canonical capability vocabulary.
///
/// Named actions are matched case- and separator-insensitively because
/// toolkits spell them differently (`click`, `press`, `activate`, GTK's
/// `expand or contract`, Chromium's `showContextMenu`). An action name this
/// map does not know contributes nothing rather than a guess. Chromium's
/// `doDefault` is on every node, panels and text runs included, so like
/// UIA's `LegacyIAccessible` it is no evidence of an affordance; the roles
/// whose real action it is (tabs, links) are interactive by role.
pub(crate) fn available_actions(evidence: &AffordanceEvidence<'_>) -> Vec<String> {
    let mut actions = Vec::new();
    for name in evidence.action_names {
        for capability in named_capabilities(&normalize(name)) {
            push_unique(&mut actions, capability);
        }
    }
    if evidence.editable_text && !evidence.states.read_only() {
        push_unique(&mut actions, capability::SET_VALUE);
    }
    if evidence.settable_value && !evidence.states.read_only() {
        push_unique(&mut actions, capability::SET_VALUE);
    }
    if evidence.states.selectable() {
        push_unique(&mut actions, capability::SELECT);
    }
    if evidence.states.focusable() {
        push_unique(&mut actions, capability::SET_FOCUS);
    }
    actions
}

fn named_capabilities(name: &str) -> &'static [&'static str] {
    match name {
        "click" | "press" | "activate" | "jump" | "open" | "invoke" => &[capability::CLICK],
        "toggle" | "check" | "uncheck" => &[capability::TOGGLE],
        "select" => &[capability::SELECT],
        "expandorcontract" | "expandcollapse" | "openclose" => {
            &[capability::EXPAND, capability::COLLAPSE]
        }
        "expand" => &[capability::EXPAND],
        "collapse" => &[capability::COLLAPSE],
        "showmenu" | "showcontextmenu" | "popup" | "menu" => &[capability::RIGHT_CLICK],
        "scrollto" | "scrollintoview" => &[capability::SCROLL_TO],
        _ => &[],
    }
}

fn normalize(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn push_unique(actions: &mut Vec<String>, capability: &str) {
    if !actions.iter().any(|existing| existing == capability) {
        actions.push(capability.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence<'a>(names: &'a [String], states: AtspiStates) -> AffordanceEvidence<'a> {
        AffordanceEvidence {
            action_names: names,
            states,
            editable_text: false,
            settable_value: false,
        }
    }

    fn states(bits: &[u32]) -> AtspiStates {
        let mut words = [0_u32; 2];
        for bit in bits {
            words[(*bit / 32) as usize] |= 1 << (bit % 32);
        }
        AtspiStates::from_words(&words)
    }

    #[test]
    fn toolkit_spellings_of_activation_map_to_click() {
        for name in ["click", "Press", "activate", "jump"] {
            let names = [name.to_string()];
            assert_eq!(available_actions(&evidence(&names, states(&[]))), ["Click"]);
        }
    }

    #[test]
    fn gtk_expand_or_contract_offers_both_directions() {
        let names = ["expand or contract".to_string()];
        assert_eq!(
            available_actions(&evidence(&names, states(&[]))),
            ["Expand", "Collapse"]
        );
    }

    #[test]
    fn chromium_check_and_uncheck_are_a_toggle() {
        for name in ["check", "uncheck"] {
            let names = [name.to_string()];
            assert_eq!(
                available_actions(&evidence(&names, states(&[]))),
                ["Toggle"]
            );
        }
    }

    #[test]
    fn chromium_do_default_is_not_an_affordance() {
        let names = ["doDefault".to_string(), "showContextMenu".to_string()];
        assert_eq!(
            available_actions(&evidence(&names, states(&[]))),
            ["RightClick"]
        );
    }

    #[test]
    fn an_unknown_action_name_contributes_nothing() {
        let names = ["frobnicate".to_string()];
        assert!(available_actions(&evidence(&names, states(&[]))).is_empty());
    }

    #[test]
    fn editable_text_is_settable_unless_read_only() {
        let mut writable = evidence(&[], states(&[]));
        writable.editable_text = true;
        assert_eq!(available_actions(&writable), ["SetValue"]);
        let mut read_only = evidence(&[], states(&[43]));
        read_only.editable_text = true;
        assert!(available_actions(&read_only).is_empty());
    }

    #[test]
    fn focus_alone_is_reported_but_selection_follows_the_state() {
        assert_eq!(
            available_actions(&evidence(&[], states(&[11, 22]))),
            ["Select", "SetFocus"]
        );
    }
}
