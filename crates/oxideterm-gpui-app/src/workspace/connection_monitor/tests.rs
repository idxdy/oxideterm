use super::*;

#[test]
fn host_tools_connection_row_only_enables_switching_when_possible() {
    assert!(!monitor_connection_can_switch(&connection_options(0)));
    assert!(!monitor_connection_can_switch(&connection_options(1)));
    assert!(monitor_connection_can_switch(&connection_options(2)));
}

fn connection_options(count: usize) -> Vec<MonitorConnectionOption> {
    (0..count)
        .map(|index| MonitorConnectionOption {
            connection_id: format!("conn-{index}"),
            host: format!("host-{index}"),
            port: 22,
            username: "user".to_string(),
            label: None,
        })
        .collect()
}

fn redis_endpoint(target: &str) -> oxideterm_connection_monitor::ResourceRedisEndpoint {
    oxideterm_connection_monitor::ResourceRedisEndpoint {
        kind: oxideterm_connection_monitor::ResourceRedisEndpointKind::Host,
        target: target.to_string(),
    }
}

fn redis_keys_request(connection_id: &str, target: &str) -> HostRedisKeysRequest {
    HostRedisKeysRequest {
        connection_id: connection_id.to_string(),
        endpoint: redis_endpoint(target),
        password: zeroize::Zeroizing::new(String::new()),
        pattern: None,
        failure_fallback: "failed".to_string(),
    }
}

fn redis_value_request(
    connection_id: &str,
    target: &str,
    database: u32,
    key: &str,
    purpose: HostRedisValuePurpose,
) -> HostRedisValueRequest {
    HostRedisValueRequest {
        connection_id: connection_id.to_string(),
        endpoint: redis_endpoint(target),
        database,
        key: key.to_string(),
        password: zeroize::Zeroizing::new(String::new()),
        purpose,
        failure_fallback: "failed".to_string(),
    }
}

fn redis_value_dialog(
    connection_id: &str,
    target: &str,
    database: u32,
    key: &str,
) -> HostRedisValueDialog {
    HostRedisValueDialog {
        connection_id: connection_id.to_string(),
        endpoint: redis_endpoint(target),
        database,
        key: key.to_string(),
        value: None,
        loading: true,
        error: None,
        json_view: false,
    }
}

#[test]
fn redis_keys_request_is_stale_once_the_selection_moved() {
    let request = redis_keys_request("conn-a", "127.0.0.1:6379");
    assert!(!request.is_stale_for(Some("conn-a"), Some(&redis_endpoint("127.0.0.1:6379"))));
    // Another host, another endpoint, and no selection all invalidate a queued
    // read that was made for the old target.
    assert!(request.is_stale_for(Some("conn-b"), Some(&redis_endpoint("127.0.0.1:6379"))));
    assert!(request.is_stale_for(Some("conn-a"), Some(&redis_endpoint("127.0.0.1:6380"))));
    assert!(request.is_stale_for(None, None));
}

#[test]
fn redis_value_dialog_only_accepts_the_delivery_for_its_own_target() {
    let dialog = redis_value_dialog("conn-a", "127.0.0.1:6379", 0, "session:1");
    assert!(dialog.matches_request(&redis_value_request(
        "conn-a",
        "127.0.0.1:6379",
        0,
        "session:1",
        HostRedisValuePurpose::Open,
    )));
    // A key name is not unique across hosts, endpoints, or databases.
    for request in [
        redis_value_request(
            "conn-b",
            "127.0.0.1:6379",
            0,
            "session:1",
            HostRedisValuePurpose::Open,
        ),
        redis_value_request(
            "conn-a",
            "127.0.0.1:6380",
            0,
            "session:1",
            HostRedisValuePurpose::Open,
        ),
        redis_value_request(
            "conn-a",
            "127.0.0.1:6379",
            1,
            "session:1",
            HostRedisValuePurpose::Open,
        ),
        redis_value_request(
            "conn-a",
            "127.0.0.1:6379",
            0,
            "session:2",
            HostRedisValuePurpose::Open,
        ),
        // A copy read never belongs to the dialog, even for the same key.
        redis_value_request(
            "conn-a",
            "127.0.0.1:6379",
            0,
            "session:2",
            HostRedisValuePurpose::Open,
        ),
    ] {
        assert!(!dialog.matches_request(&request));
    }
    // Purpose is not part of the target identity: the receiver decides what a
    // copy or verify read may do with the dialog.
    assert!(dialog.matches_request(&redis_value_request(
        "conn-a",
        "127.0.0.1:6379",
        0,
        "session:1",
        HostRedisValuePurpose::VerifyWrite,
    )));
}

fn secret_redis_value(body: &str) -> oxideterm_connection_monitor::ResourceRedisValue {
    oxideterm_connection_monitor::ResourceRedisValue {
        status: oxideterm_connection_monitor::ResourceRedisStatus::Available,
        kind: oxideterm_connection_monitor::ResourceRedisValueKind::Hash,
        raw_type: Some("Timeseries".to_string()),
        ttl: oxideterm_connection_monitor::ResourceRedisTtl::Never,
        memory_usage: Some(64),
        length: Some(2),
        body: body.to_string(),
        elements: vec![b"field".to_vec(), body.as_bytes().to_vec()],
    }
}

#[test]
fn redis_value_wipe_clears_every_secret_bearing_projection() {
    let mut value = secret_redis_value("secret");
    zeroize_redis_value(&mut value);
    assert!(value.body.is_empty());
    assert!(value.elements.is_empty());
    assert_eq!(value.raw_type.as_deref(), Some(""));
    // Metadata that is not secret stays readable for rendering.
    assert_eq!(value.length, Some(2));
    assert_eq!(value.memory_usage, Some(64));
}

#[test]
fn redis_value_dialog_replaces_and_clears_through_the_wiping_path() {
    let mut dialog = redis_value_dialog("conn-a", "127.0.0.1:6379", 0, "session:1");
    dialog.set_value(Some(secret_redis_value("first")));
    assert_eq!(
        dialog.value.as_ref().map(|value| value.body.as_str()),
        Some("first")
    );
    dialog.set_value(Some(secret_redis_value("second")));
    assert_eq!(
        dialog.value.as_ref().map(|value| value.body.as_str()),
        Some("second")
    );
    dialog.clear_value();
    assert!(dialog.value.is_none());
    dialog.set_value(None);
    assert!(dialog.value.is_none());
}

#[test]
fn invalidating_the_container_text_dialog_retires_its_running_capture() {
    let mut state = HostContainerState::new();
    let first = state.begin_container_text_dialog();
    let second = state.begin_container_text_dialog();
    assert_ne!(first, second);

    state.text_dialog_running_generation = Some(second);
    // No dialog was open, so nothing gets reported as closed.
    assert!(!state.invalidate_container_text_dialog());
    assert_ne!(state.text_dialog_generation, second);
    assert!(state.text_dialog_running_generation.is_none());
    assert!(state.text_dialog_running.is_none());
}
