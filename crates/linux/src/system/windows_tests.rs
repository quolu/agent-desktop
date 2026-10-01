use super::*;

#[test]
fn window_ids_are_stable_and_distinguish_paths() {
    let first = window_id(4242, "/org/a11y/atspi/accessible/1");
    assert_eq!(first, window_id(4242, "/org/a11y/atspi/accessible/1"));
    assert_ne!(first, window_id(4242, "/org/a11y/atspi/accessible/2"));
    assert_ne!(first, window_id(4243, "/org/a11y/atspi/accessible/1"));
    assert!(first.starts_with("w-4242-"));
}

#[test]
fn wayland_window_relative_extents_are_not_screen_coordinates() {
    let origin = Some((0, 0, 630, 267));
    assert!(!coordinates_trusted(Some("gtk"), origin, origin));
    assert!(!coordinates_trusted(Some("clutter"), origin, origin));
}

#[test]
fn chromium_coordinates_are_trusted_even_at_the_origin() {
    let origin = Some((0, 0, 2493, 1408));
    assert!(coordinates_trusted(Some("Chromium"), origin, origin));
}

#[test]
fn screen_extents_that_differ_from_window_extents_are_real() {
    assert!(coordinates_trusted(
        Some("gtk"),
        Some((166, 152, 1788, 1121)),
        Some((0, 0, 1788, 1121))
    ));
}

#[test]
fn missing_screen_extents_are_never_trusted() {
    assert!(!coordinates_trusted(
        Some("Chromium"),
        None,
        Some((0, 0, 10, 10))
    ));
}
