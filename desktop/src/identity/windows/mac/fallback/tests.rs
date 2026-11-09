use super::*;
use std::cell::Cell;

const IP: &[i8] = &[b'4' as i8, 0];
const OTHER_IP: &[i8] = &[b'5' as i8, 0];
const NAME: &[i8] = &[b'a' as i8, 0];
const ADDRESS: &[u8] = &[2, 1, 3, 0xff, 0x10, 0xaa, 99, 88];
const HOST: u32 = 4;

fn matching() -> Adapter<'static> {
    Adapter {
        kind: ETHERNET,
        first_ip: IP,
        name: NAME,
        address: ADDRESS,
        address_length: 6,
    }
}

fn parse(ip: &[u8]) -> Option<u32> {
    match ip {
        b"4\0" => Some(HOST),
        b"5\0" => Some(5),
        _ => None,
    }
}

fn convert(name: &[u8]) -> Option<Vec<u16>> {
    (name == b"a\0").then(|| vec![u16::from(b'a')])
}

#[test]
fn first_matching_ethernet_uses_permanent_reply_and_stops() {
    let calls = Cell::new(0);
    let result = select(
        HOST,
        [
            Ok(Adapter {
                first_ip: OTHER_IP,
                ..matching()
            }),
            Ok(Adapter {
                kind: 71,
                ..matching()
            }),
            Ok(matching()),
            Err(FallbackError::AdapterList),
        ],
        parse,
        convert,
        |path| {
            assert_eq!(path, device_path(&[u16::from(b'a')]).unwrap());
            calls.set(calls.get() + 1);
            DriverReply::Bytes(MacAddress([2, 4, 6, 8, 10, 12]))
        },
    );
    assert!(matches!(
        result,
        Ok(FallbackObservation::Available(FallbackSource::Ndis, address))
            if address.official_ascii() == *b"02:04:06:08:0A:0C"
    ));
    assert_eq!(calls.get(), 1);
}

#[test]
fn definite_driver_failure_reads_only_six_attested_adapter_bytes() {
    let result = select(
        HOST,
        [Ok(Adapter {
            address_length: 8,
            ..matching()
        })],
        parse,
        convert,
        |_| DriverReply::Failed,
    );
    assert!(matches!(
        result,
        Ok(FallbackObservation::Available(FallbackSource::AdapterRecord, address))
            if address.official_ascii() == *b"02:01:03:FF:10:AA"
    ));
    for address_length in [0, 5, 9, u32::MAX] {
        let result = select(
            HOST,
            [Ok(Adapter {
                address_length,
                ..matching()
            })],
            parse,
            convert,
            |_| DriverReply::Failed,
        );
        assert!(matches!(result, Err(FallbackError::AddressLength)));
    }
}

#[test]
fn malformed_driver_reply_and_outer_filter_do_not_select_replacements() {
    assert!(matches!(
        select(HOST, [Ok(matching())], parse, convert, |_| {
            DriverReply::Malformed
        }),
        Err(FallbackError::DriverReply)
    ));
    let result = select(
        HOST,
        [Ok(matching()), Err(FallbackError::AdapterList)],
        parse,
        convert,
        |_| DriverReply::Bytes(MacAddress([255; 6])),
    );
    assert!(matches!(
        result,
        Ok(FallbackObservation::RejectedByOuterFilter)
    ));
    let result = select(
        HOST,
        [Ok(Adapter {
            address: &[0, 0, 0, 0, 0, 3],
            ..matching()
        })],
        parse,
        convert,
        |_| DriverReply::Failed,
    );
    assert!(matches!(
        result,
        Ok(FallbackObservation::RejectedByOuterFilter)
    ));
}

#[test]
fn unmatchable_ip_and_unsafe_or_unverified_names_never_query_a_device() {
    let unexpected_query = |_: &[u16]| panic!("unexpected device query");
    assert!(matches!(
        select(8, [Ok(matching())], parse, convert, unexpected_query),
        Ok(FallbackObservation::NoMatch)
    ));
    assert!(matches!(
        select(
            HOST,
            [Ok(Adapter {
                first_ip: &[b'4' as i8],
                ..matching()
            })],
            parse,
            convert,
            unexpected_query,
        ),
        Err(FallbackError::AdapterIp)
    ));
    assert!(matches!(
        select(HOST, [Ok(matching())], parse, |_| None, unexpected_query),
        Err(FallbackError::NameConversion)
    ));
    assert!(matches!(
        select(
            HOST,
            [Ok(matching())],
            parse,
            |_| Some(r"wrong\device".encode_utf16().collect()),
            unexpected_query,
        ),
        Err(FallbackError::DeviceName)
    ));
}

#[test]
fn list_positions_accept_original_order_and_reject_cycles_overlap_or_escape() {
    let mut records = Records::new(0x1000, 0x300, 0x100, 8);
    assert_eq!(records.next_offset(0x1000), Ok(0));
    assert_eq!(records.next_offset(0x1200), Ok(0x200));
    assert_eq!(records.next_offset(0x1100), Ok(0x100));
    for address in [
        0,
        0x0ff8,
        0x1000,
        0x1001,
        0x1008,
        0x12f8,
        0x1300,
        usize::MAX,
    ] {
        assert_eq!(
            records.next_offset(address),
            Err(FallbackError::AdapterList)
        );
    }
}

#[test]
fn checked_short_conversion_requires_every_unit_and_both_guards() {
    let control = [0x0041, 0xd83d, 0xde42, 0xffff, 0xa55a];
    let mut short = short_name_buffer(&control);
    assert!(verified_short_name(&short, &control).is_none());
    short[1..control.len()].copy_from_slice(&control[..control.len() - 1]);
    assert!(verified_short_name(&short, &control).is_none());
    short[control.len()] = control[control.len() - 1];
    assert_eq!(
        verified_short_name(&short, &control),
        Some(control.as_slice())
    );
    short[control.len() + 1] = 0;
    assert!(verified_short_name(&short, &control).is_none());
}
