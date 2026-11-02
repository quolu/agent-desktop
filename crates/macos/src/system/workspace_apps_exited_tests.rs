use super::*;

fn snapshot() -> &'static [u8] {
    br#"{
        "applications":[
            {"name":"Target","pid":10,"launch_time":100.25,"activation_policy":"regular"},
            {"name":"Target","pid":11,"launch_time":101.25,"activation_policy":"regular"},
            {"name":"Other","pid":12,"launch_time":102.25,"activation_policy":"regular"}
        ],
        "frontmost_pid":10,
        "frontmost_launch_time":100.25
    }"#
}

#[test]
fn complete_inventory_omits_confirmed_exited_workspace_records() {
    let apps = apps_from_json_with(
        snapshot(),
        Instant::now() + std::time::Duration::from_secs(1),
        |_| true,
        true,
        |pid| Ok((pid != 11).then(|| format!("instance-{pid}"))),
    )
    .unwrap();

    assert_eq!(apps.iter().map(|app| app.pid).collect::<Vec<_>>(), [10, 12]);
    assert_eq!(apps[0].process_instance.as_deref(), Some("instance-10"));
    assert_eq!(apps[1].process_instance.as_deref(), Some("instance-12"));
}

#[test]
fn scoped_inventory_keeps_the_live_duplicate_of_an_exited_app() {
    let apps = apps_from_json_with(
        snapshot(),
        Instant::now() + std::time::Duration::from_secs(1),
        |app| app.name == "Target",
        false,
        |pid| Ok((pid != 11).then(|| format!("instance-{pid}"))),
    )
    .unwrap();

    assert_eq!(apps.iter().map(|app| app.pid).collect::<Vec<_>>(), [10]);
}

#[test]
fn inaccessible_live_identity_still_fails_the_inventory() {
    let error = apps_from_json_with(
        snapshot(),
        Instant::now() + std::time::Duration::from_secs(1),
        |_| true,
        true,
        |pid| match pid {
            10 => Ok(None),
            11 => Err(AdapterError::permission_denied()),
            _ => Ok(Some(format!("instance-{pid}"))),
        },
    )
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::PermDenied);
}
