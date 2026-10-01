use agent_desktop_core::Role;

use super::states::AtspiStates;

const ROLE_FILLER: u32 = 20;
const ROLE_PANEL: u32 = 39;
const ROLE_SECTION: u32 = 85;
const ROLE_PASSWORD_TEXT: u32 = 40;

/// Maps an AT-SPI2 role number (`AtspiRole`, at-spi2-core 2.60) to the
/// canonical role, refined by the states read in the same pass.
///
/// The role is read as its raw number rather than a decoded enum so a role a
/// newer toolkit adds is a readable `unknown` rather than a failed read.
pub(crate) fn canonical_role(code: u32, states: &AtspiStates) -> Role {
    match code {
        2 => Role::Alert,
        3 | 4 | 26 | 27 | 80 | 100 => Role::Image,
        7 => Role::Checkbox,
        8 | 35 | 45 | 59 => Role::MenuItem,
        9 => Role::ColorWell,
        10 | 57 => Role::Column,
        11 | 76 => Role::ComboBox,
        12 => Role::DateField,
        13 => Role::Button,
        14 | 23 | 69 | 89 => Role::Window,
        15 | 51 => Role::Slider,
        16 | 19 | 22 | 36 => Role::Dialog,
        1 | 29 | 81 | 116 => Role::StaticText,
        31 | 121 => Role::List,
        32 => list_item_role(states),
        33 | 34 | 41 => Role::Menu,
        37 => Role::Tab,
        38 => Role::TabList,
        40 | 60 | 77 | 79 => Role::TextField,
        42 => Role::ProgressBar,
        43 | 62 => Role::Button,
        44 => Role::RadioButton,
        47 | 56 | 58 => Role::Cell,
        48 => Role::ScrollBar,
        49 => Role::ScrollArea,
        50 => Role::Separator,
        52 => Role::Incrementor,
        53 => Role::Splitter,
        54 | 102 => Role::Status,
        55 => Role::Table,
        61 => text_role(states),
        63 => Role::Toolbar,
        64 => Role::Tooltip,
        65 | 66 => Role::Outline,
        71 => Role::Banner,
        72 => Role::ContentInfo,
        73 => Role::Paragraph,
        74 => Role::Ruler,
        75 => Role::Application,
        82 | 92 | 93 | 94 | 96 => Role::Document,
        83 => Role::Heading,
        84 => Role::Page,
        87 => Role::Form,
        88 => Role::Link,
        90 => Role::Row,
        91 => Role::TreeItem,
        95 => Role::WebArea,
        97 | 124 => Role::Note,
        98 => Role::ListBox,
        101 => Role::Alert,
        103 | 114 => Role::LevelIndicator,
        108 | 123 => Role::Definition,
        109 => Role::Article,
        110 => Role::Region,
        111 => Role::Log,
        112 => Role::Marquee,
        113 | 117 | 118 => Role::Math,
        115 => Role::Timer,
        122 => Role::Term,
        129 => Role::MenuButton,
        130 => Role::Switch,
        5 | 6 | 17 | 18 | 20 | 21 | 24 | 25 | 28 | 30 | 39 | 46 | 68 | 78 | 85 | 86 | 99 | 104
        | 105 | 106 | 107 | 119 | 120 | 125 | 126 | 127 | 128 => Role::Group,
        _ => Role::Unknown,
    }
}

/// A list item is a selectable option inside a list box, and a plain content
/// item (an HTML `<li>`) everywhere else; toolkits use the same role for both.
fn list_item_role(states: &AtspiStates) -> Role {
    if states.selectable() {
        Role::Option
    } else {
        Role::Group
    }
}

/// `ROLE_TEXT` covers both an editable text view and a read-only text run.
fn text_role(states: &AtspiStates) -> Role {
    if states.editable() {
        Role::TextField
    } else {
        Role::StaticText
    }
}

/// Non-semantic wrappers that do not consume depth budget when they carry no
/// name: the AT-SPI2 counterpart of the macOS `AXGroup` and Windows pane skip.
pub(crate) fn is_wrapper_role(code: u32) -> bool {
    matches!(code, ROLE_FILLER | ROLE_PANEL | ROLE_SECTION)
}

/// Ranges a user can change: dial, scroll bar, slider, spin button. A progress
/// bar or level bar exposes the same `Value` interface but is read-only.
pub(crate) fn is_adjustable_role(code: u32) -> bool {
    matches!(code, 15 | 48 | 51 | 52)
}

pub(crate) fn is_password_role(code: u32) -> bool {
    code == ROLE_PASSWORD_TEXT
}

#[cfg(test)]
#[path = "roles_tests.rs"]
mod tests;
