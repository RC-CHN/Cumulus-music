use super::*;
use std::{
    ffi::CString,
    mem::{align_of, offset_of, size_of},
    ptr,
};

fn read(value: &str) -> Result<Vec<OutputDevice>> {
    let value = CString::new(value).unwrap();
    let mut command = Command {
        result: Node {
            value: Value {
                string: value.as_ptr(),
            },
            format: STRING,
        },
    };
    // SAFETY: the fixture owns the event string for this entire call.
    unsafe { copy((&mut command as *mut Command).cast()) }
}
#[test]
fn pinned_64_bit_command_layout() {
    assert_eq!(size_of::<Node>(), 16);
    assert_eq!(align_of::<Node>(), 8);
    assert_eq!(offset_of!(Node, format), 8);
    assert_eq!(size_of::<Command>(), 16);
    assert_eq!(offset_of!(Command, result), 0);
}
#[test]
fn copies_opaque_names_and_unicode_without_normalizing_and_outlives_event() {
    let result = read(r#"[{"name":"auto","description":"系统默认"},{"name":"wasapi/{A-b}\\one\"two","description":"USB 耳机","future":{"nested":[1,true]}}]"#).unwrap();
    assert_eq!(
        result,
        vec![
            OutputDevice {
                id: "auto".into(),
                description: "系统默认".into()
            },
            OutputDevice {
                id: "wasapi/{A-b}\\one\"two".into(),
                description: "USB 耳机".into()
            },
        ]
    );
    assert_eq!(read("[]").unwrap(), vec![]);
}
#[test]
fn rejects_nulls_and_wrong_tags_before_accessing_payload() {
    // SAFETY: NULL is explicitly rejected before dereference.
    assert!(unsafe { copy(ptr::null_mut()) }.is_err());
    for format in [0, 6, 7, 8, 99, STRING] {
        let mut command = Command {
            result: Node {
                format,
                value: Value {
                    string: ptr::null(),
                },
            },
        };
        // SAFETY: live fixture; the pointer/tag must be checked before use.
        assert!(unsafe { copy((&mut command as *mut Command).cast()) }.is_err());
    }
}
#[test]
fn rejects_invalid_json_types_duplicate_fields_and_ambiguous_ids() {
    for text in [
        "",
        "(unavailable)",
        "null",
        "{}",
        "[true]",
        "[] trailing",
        r#"[{"name":"auto"}]"#,
        r#"[{"name":1,"description":"one"}]"#,
        r#"[{"name":"auto","name":"other","description":"one"}]"#,
        r#"[{"name":"auto","description":"one","description":"two"}]"#,
        r#"[{"name":"auto","description":"one"},{"name":"auto","description":"two"}]"#,
        r#"[{"name":"","description":"one"}]"#,
        r#"[{"name":"a\u0000b","description":"one"}]"#,
        r#"[{"name":"auto","description":"a\u0000b"}]"#,
    ] {
        assert!(read(text).is_err(), "accepted {text}");
    }
    assert!(parse(b"[\xff]").is_err());
}
#[test]
fn bounds_device_count_ids_labels_and_reply_bytes() {
    let list: Vec<_> = (0..1024)
        .map(|n| serde_json::json!({"name":format!("device/{n}"),"description":"label"}))
        .collect();
    assert_eq!(
        parse(&serde_json::to_vec(&list).unwrap()).unwrap().len(),
        1024
    );
    let mut extra = list;
    extra.push(serde_json::json!({"name":"extra","description":"label"}));
    assert!(parse(&serde_json::to_vec(&extra).unwrap()).is_err());
    for (name, description) in [
        ("a".repeat(4097), "label".into()),
        ("auto".into(), "x".repeat(16385)),
    ] {
        assert!(
            parse(
                &serde_json::to_vec(&serde_json::json!([{"name":name,"description":description}]))
                    .unwrap()
            )
            .is_err()
        );
    }
    assert!(parse(&vec![b' '; MAX_JSON + 1]).is_err());
}

#[test]
fn command_replies_keep_current_query_separate_from_late_and_unrelated_replies() {
    use super::super::{Event, Received, command_reply};

    let mut query = Some(42);
    let mut event = Event {
        event_id: 5,
        error: 0,
        reply_userdata: 41,
        data: ptr::null_mut(),
    };
    // SAFETY: command reply fixture. Nonmatching payloads must not be read.
    assert!(matches!(
        unsafe { command_reply(&mut query, &event) },
        Received::Reply {
            sequence: 41,
            error: 0
        }
    ));
    assert_eq!(query, Some(42));

    let json = CString::new(r#"[{"name":"auto","description":"System default"}]"#).unwrap();
    let mut payload = Command {
        result: Node {
            value: Value {
                string: json.as_ptr(),
            },
            format: STRING,
        },
    };
    event.reply_userdata = 42;
    event.data = (&mut payload as *mut Command).cast();
    // SAFETY: the matching fixture owns the command payload and its string.
    let Received::Devices { sequence, result } = (unsafe { command_reply(&mut query, &event) })
    else {
        panic!("query reply lost");
    };
    assert_eq!(sequence, 42);
    assert_eq!(result.unwrap()[0].id, "auto");
    assert_eq!(query, None);

    event.data = ptr::null_mut();
    // SAFETY: duplicate/ordinary command reply; payload must not be read.
    assert!(matches!(
        unsafe { command_reply(&mut query, &event) },
        Received::Reply { .. }
    ));
    query = Some(43);
    event.reply_userdata = 43;
    event.error = -10;
    // SAFETY: failed matching reply has no payload and must not be dereferenced.
    assert!(matches!(
        unsafe { command_reply(&mut query, &event) },
        Received::Devices {
            sequence: 43,
            result: Err(Error::Playback { code: -10, .. })
        }
    ));
    assert_eq!(query, None);
}
