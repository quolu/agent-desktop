use agent_desktop_core::state;

const ACTIVE: u32 = 1;
const BUSY: u32 = 3;
const CHECKED: u32 = 4;
const EDITABLE: u32 = 7;
const ENABLED: u32 = 8;
const EXPANDED: u32 = 10;
const FOCUSABLE: u32 = 11;
const FOCUSED: u32 = 12;
const ICONIFIED: u32 = 15;
const MODAL: u32 = 16;
const MULTISELECTABLE: u32 = 18;
const PRESSED: u32 = 20;
const SELECTABLE: u32 = 22;
const SELECTED: u32 = 23;
const SENSITIVE: u32 = 24;
const SHOWING: u32 = 25;
const VISIBLE: u32 = 30;
const INDETERMINATE: u32 = 32;
const REQUIRED: u32 = 33;
const INVALID_ENTRY: u32 = 36;
const HAS_POPUP: u32 = 42;
const READ_ONLY: u32 = 43;

/// The AT-SPI2 state set as the two 32-bit words `GetState` returns
/// (`AtspiStateType` bit positions, at-spi2-core 2.60).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AtspiStates(u64);

impl AtspiStates {
    pub(crate) fn from_words(words: &[u32]) -> Self {
        let low = words.first().copied().unwrap_or(0);
        let high = words.get(1).copied().unwrap_or(0);
        Self(u64::from(low) | (u64::from(high) << 32))
    }

    fn has(self, bit: u32) -> bool {
        self.0 & (1_u64 << bit) != 0
    }

    pub(crate) fn active(self) -> bool {
        self.has(ACTIVE)
    }

    pub(crate) fn editable(self) -> bool {
        self.has(EDITABLE)
    }

    pub(crate) fn focusable(self) -> bool {
        self.has(FOCUSABLE)
    }

    pub(crate) fn iconified(self) -> bool {
        self.has(ICONIFIED)
    }

    pub(crate) fn selectable(self) -> bool {
        self.has(SELECTABLE)
    }

    pub(crate) fn read_only(self) -> bool {
        self.has(READ_ONLY)
    }

    pub(crate) fn showing(self) -> bool {
        self.has(SHOWING)
    }

    pub(crate) fn visible(self) -> bool {
        self.has(VISIBLE)
    }

    /// AT-SPI2 marks a usable control `ENABLED` and `SENSITIVE`; a control
    /// carrying neither is greyed out. An object that sets only one of the two
    /// is not treated as disabled, so a toolkit that publishes one flag cannot
    /// make every node look unusable.
    pub(crate) fn disabled(self) -> bool {
        !self.has(ENABLED) && !self.has(SENSITIVE)
    }

    /// Projects the set onto the canonical state vocabulary.
    pub(crate) fn canonical(self, secure: bool) -> Vec<String> {
        let mut states = Vec::new();
        let mut push = |present: bool, token: &str| {
            if present {
                states.push(token.to_string());
            }
        };
        push(self.has(FOCUSED), state::FOCUSED);
        push(self.disabled(), state::DISABLED);
        push(secure, state::SECURE);
        push(self.has(EXPANDED), state::EXPANDED);
        push(self.has(CHECKED), state::CHECKED);
        push(self.has(SELECTED), state::SELECTED);
        push(!self.visible(), state::HIDDEN);
        push(self.has(BUSY), state::BUSY);
        push(self.has(MODAL), state::MODAL);
        push(self.has(REQUIRED), state::REQUIRED);
        push(self.has(INDETERMINATE), state::INDETERMINATE);
        push(self.has(PRESSED), state::PRESSED);
        push(self.read_only(), state::READONLY);
        push(self.visible() && !self.showing(), state::OFFSCREEN);
        push(self.has(INVALID_ENTRY), state::INVALID);
        push(self.has(MULTISELECTABLE), state::MULTISELECTABLE);
        push(self.has(HAS_POPUP), state::HASPOPUP);
        states
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(bits: &[u32]) -> AtspiStates {
        let mut words = [0_u32; 2];
        for bit in bits {
            words[(*bit / 32) as usize] |= 1 << (bit % 32);
        }
        AtspiStates::from_words(&words)
    }

    #[test]
    fn an_ordinary_visible_control_has_no_states() {
        let states = with(&[ENABLED, SENSITIVE, SHOWING, VISIBLE, FOCUSABLE]);
        assert!(states.canonical(false).is_empty());
    }

    #[test]
    fn states_in_the_high_word_are_read() {
        let states = with(&[ENABLED, SHOWING, VISIBLE, HAS_POPUP, READ_ONLY]);
        assert_eq!(states.canonical(false), [state::READONLY, state::HASPOPUP]);
    }

    #[test]
    fn neither_enabled_nor_sensitive_is_disabled() {
        assert_eq!(
            with(&[SHOWING, VISIBLE]).canonical(false),
            [state::DISABLED]
        );
        assert!(!with(&[SENSITIVE, SHOWING, VISIBLE]).disabled());
    }

    #[test]
    fn visible_but_not_showing_is_offscreen_and_invisible_is_hidden() {
        let scrolled_away = with(&[ENABLED, VISIBLE]);
        assert_eq!(scrolled_away.canonical(false), [state::OFFSCREEN]);
        let hidden = with(&[ENABLED]);
        assert_eq!(hidden.canonical(false), [state::HIDDEN]);
    }

    #[test]
    fn every_emitted_state_is_in_the_vocabulary() {
        let all = AtspiStates::from_words(&[u32::MAX, u32::MAX]);
        state::assert_states_in_vocabulary(&all.canonical(true));
        let none = AtspiStates::from_words(&[]);
        state::assert_states_in_vocabulary(&none.canonical(true));
    }
}
