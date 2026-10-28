use super::*;
use std::{
    ffi::CString,
    mem::{align_of, offset_of, size_of},
    ptr,
};

fn fixture<T>(names: &[(&str, &str)], use_root: impl FnOnce(&mut Node) -> T) -> T {
    let strings: Vec<_> = names
        .iter()
        .map(|(id, label)| [CString::new(*id).unwrap(), CString::new(*label).unwrap()])
        .collect();
    let mut keys = [
        c"name".as_ptr().cast_mut(),
        c"description".as_ptr().cast_mut(),
    ];
    let mut fields: Vec<_> = strings
        .iter()
        .map(|values| {
            values.each_ref().map(|v| Node {
                value: Value {
                    string: v.as_ptr().cast_mut(),
                },
                format: STRING,
            })
        })
        .collect();
    let mut maps: Vec<_> = fields
        .iter_mut()
        .map(|values| List {
            num: 2,
            values: values.as_mut_ptr(),
            keys: keys.as_mut_ptr(),
        })
        .collect();
    let mut items: Vec<_> = maps
        .iter_mut()
        .map(|map| Node {
            format: MAP,
            value: Value { list: map },
        })
        .collect();
    let mut array = List {
        num: items.len() as c_int,
        values: items.as_mut_ptr(),
        keys: ptr::null_mut(),
    };
    use_root(&mut Node {
        format: ARRAY,
        value: Value { list: &mut array },
    })
}
fn read(root: &mut Node) -> Result<Vec<OutputDevice>> {
    let mut property = Property {
        name: c"audio-device-list".as_ptr(),
        format: NODE,
        data: (root as *mut Node).cast(),
    };
    // SAFETY: fixture owns every pointer until this call has completed.
    unsafe { copy((&mut property as *mut Property).cast()) }
}
#[test]
fn pinned_64_bit_node_layout() {
    // Both supported x86_64 ABIs use these sizes, including Windows LLP64.
    assert_eq!(size_of::<Node>(), 16);
    assert_eq!(align_of::<Node>(), 8);
    assert_eq!(offset_of!(Node, format), 8);
    assert_eq!(size_of::<List>(), 24);
    assert_eq!(offset_of!(List, values), 8);
    assert_eq!(offset_of!(List, keys), 16);
    assert_eq!(size_of::<Property>(), 24);
    assert_eq!(offset_of!(Property, data), 16);
}
#[test]
fn copies_names_without_normalizing_and_outlives_event() {
    let result = fixture(&[("auto", "系统默认"), ("wasapi/{A-b}", "USB 耳机")], read).unwrap();
    assert_eq!(
        result,
        vec![
            OutputDevice {
                id: "auto".into(),
                description: "系统默认".into()
            },
            OutputDevice {
                id: "wasapi/{A-b}".into(),
                description: "USB 耳机".into()
            }
        ]
    );
}
#[test]
fn empty_array_accepts_null_values_without_constructing_null_slice() {
    fixture(&[], |root| {
        // SAFETY: these are exclusively owned fixture fields.
        unsafe {
            (*root.value.list).values = ptr::null_mut();
        }
        assert_eq!(read(root).unwrap(), vec![]);
    });
}
#[test]
fn rejects_tags_nulls_and_invalid_counts_before_reading_payloads() {
    let mut root = Node {
        format: STRING,
        value: Value {
            string: ptr::null_mut(),
        },
    };
    assert!(read(&mut root).is_err());
    root.format = ARRAY;
    assert!(read(&mut root).is_err());
    for num in [-1, 1025, 1] {
        let mut list = List {
            num,
            values: ptr::null_mut(),
            keys: ptr::null_mut(),
        };
        root.value = Value { list: &mut list };
        assert!(read(&mut root).is_err());
    }
    // SAFETY: NULL is explicitly rejected before payload dereference.
    assert!(unsafe { copy(ptr::null_mut()) }.is_err());
}
#[test]
fn rejects_ambiguous_names_and_missing_fields() {
    assert!(fixture(&[("auto", "one"), ("auto", "two")], read).is_err());
    assert!(fixture(&[("", "empty")], read).is_err());
    assert!(fixture(&[(&"a".repeat(4097), "too long")], read).is_err());
    fixture(&[("auto", "one")], |root| {
        // SAFETY: all nodes here are the live ARRAY/MAP fixture shape.
        unsafe {
            (*(*(*root.value.list).values).value.list).num = 1;
        }
        assert!(read(root).is_err());
    });
}
#[test]
fn rejects_null_map_keys_and_wrong_string_tags() {
    fixture(&[("auto", "one")], |root| {
        // SAFETY: live fixture ARRAY/MAP nodes, changed before parsing.
        unsafe {
            (*(*(*root.value.list).values).value.list).keys = ptr::null_mut();
        }
        assert!(read(root).is_err());
    });
    fixture(&[("auto", "one")], |root| {
        // SAFETY: exclusively owned fixture node; wrong tag is rejected
        // before its (now semantically invalid) union is accessed.
        unsafe {
            (*(*(*(*root.value.list).values).value.list).values).format = ARRAY;
        }
        assert!(read(root).is_err());
    });
}
