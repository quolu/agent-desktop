use agent_desktop_core::LocatorField;
use zbus::Connection;

use super::element::{self, EDITABLE_TEXT, ElementRef, TEXT, VALUE};
use super::node_read::NodeBasics;

/// Reads the element's current value: the `Value` interface for ranges, the
/// `Text` interface for editable text. A password field's value is never
/// read, and a text run that is not editable has no value - its content is
/// already its name.
pub(crate) async fn read_value(
    connection: &Connection,
    element: &ElementRef,
    basics: &NodeBasics,
    max_bytes: usize,
) -> LocatorField<String> {
    if basics.secure() {
        return LocatorField::Absent;
    }
    if basics.has_interface(VALUE) {
        return read_range_value(connection, element).await;
    }
    let editable = basics.has_interface(EDITABLE_TEXT)
        || basics.states.is_some_and(|states| states.editable());
    if editable && basics.has_interface(TEXT) {
        return read_text_value(connection, element, max_bytes).await;
    }
    LocatorField::Absent
}

async fn read_range_value(connection: &Connection, element: &ElementRef) -> LocatorField<String> {
    match element::float_property(connection, element, VALUE, "CurrentValue").await {
        Ok(value) => LocatorField::Known(format_number(value)),
        Err(_) => LocatorField::Unknown,
    }
}

async fn read_text_value(
    connection: &Connection,
    element: &ElementRef,
    max_bytes: usize,
) -> LocatorField<String> {
    let Ok(count) = element::int_property(connection, element, TEXT, "CharacterCount").await else {
        return LocatorField::Unknown;
    };
    if count <= 0 {
        return LocatorField::Known(String::new());
    }
    let end = count.min(i32::try_from(max_bytes).unwrap_or(i32::MAX));
    match element::call::<_, String>(connection, element, TEXT, "GetText", &(0_i32, end)).await {
        Ok(text) => LocatorField::Known(truncate_to_bytes(text, max_bytes)),
        Err(_) => LocatorField::Unknown,
    }
}

/// Toolkits store ranges as `float`, so `0.6` arrives as
/// `0.6000000238418579`; six decimals is past any value a control shows.
fn format_number(value: f64) -> String {
    let fixed = format!("{value:.6}");
    let trimmed = fixed.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn truncate_to_bytes(mut text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_numbers_print_without_a_fraction() {
        assert_eq!(format_number(50.0), "50");
        assert_eq!(format_number(-3.0), "-3");
        assert_eq!(format_number(0.25), "0.25");
        assert_eq!(format_number(f64::from(0.6_f32)), "0.6");
        assert_eq!(format_number(-0.0), "0");
    }

    #[test]
    fn truncation_never_splits_a_character() {
        assert_eq!(truncate_to_bytes("あいう".to_string(), 4), "あ");
        assert_eq!(truncate_to_bytes("abc".to_string(), 8), "abc");
    }
}
