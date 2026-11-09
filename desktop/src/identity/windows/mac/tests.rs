use super::*;
use std::cell::RefCell;

#[derive(Clone, Copy)]
struct Card {
    lower_description: Option<&'static str>,
    service: Option<&'static str>,
    reply: (bool, u32, [u8; 6]),
}

const OK: [u8; 6] = [0x02, 0x0a, 0x7b, 0xff, 0x01, 0x09];
const ORDINARY: Card = Card {
    lower_description: Some("wired"),
    service: Some("{synthetic-guid}"),
    reply: (true, 6, OK),
};

enum Entry {
    Card(Card),
    Skip,
    Malformed,
}

#[derive(Debug, PartialEq, Eq)]
enum Read {
    Entry(u32),
    Description(u32),
    Service(u32),
    Ioctl(String),
}

#[derive(Default)]
struct Fixture {
    entries: Vec<Entry>,
    read: RefCell<Vec<Read>>,
}

impl NetworkCards for Fixture {
    type Key = (u32, Card);

    fn entry(&self, index: u32) -> RegistryEntry<Self::Key> {
        self.read.borrow_mut().push(Read::Entry(index));
        match self.entries.get(index as usize) {
            Some(Entry::Card(card)) => RegistryEntry::Open((index, *card)),
            Some(Entry::Skip) => RegistryEntry::Skip,
            Some(Entry::Malformed) => RegistryEntry::Malformed,
            None => RegistryEntry::End,
        }
    }

    fn lower_description(&self, (index, card): &Self::Key) -> Option<Vec<u16>> {
        self.read.borrow_mut().push(Read::Description(*index));
        Some(card.lower_description?.encode_utf16().collect())
    }

    fn service_name(&self, (index, card): &Self::Key) -> ServiceName {
        self.read.borrow_mut().push(Read::Service(*index));
        match card.service {
            Some("malformed-fixture") => ServiceName::Malformed,
            Some(value) => ServiceName::Value(value.encode_utf16().collect()),
            None => ServiceName::Unreadable,
        }
    }

    fn permanent_address(&self, path: &[u16]) -> DriverReply {
        let name = String::from_utf16(path.strip_suffix(&[0]).unwrap()).unwrap();
        self.read.borrow_mut().push(Read::Ioctl(name.clone()));
        let leaf = name.strip_prefix(r"\\.\").unwrap();
        let card = self
            .entries
            .iter()
            .find_map(|entry| match entry {
                Entry::Card(card) if card.service == Some(leaf) => Some(card),
                _ => None,
            })
            .unwrap();
        let mut buffer = [0; NDIS_OUTPUT];
        buffer[..6].copy_from_slice(&card.reply.2);
        driver_reply(card.reply.0, card.reply.1, &buffer)
    }
}

fn fixture(entries: Vec<Entry>) -> Fixture {
    Fixture {
        entries,
        ..Fixture::default()
    }
}

#[test]
fn registry_order_filters_description_before_reading_service_and_stops_after_first_success() {
    let source = fixture(vec![
        Entry::Skip,
        Entry::Card(Card {
            lower_description: Some("found virtual hardware"),
            ..ORDINARY
        }),
        Entry::Card(Card {
            lower_description: Some("nic 1394 transport"),
            ..ORDINARY
        }),
        Entry::Card(Card {
            service: None,
            ..ORDINARY
        }),
        Entry::Card(Card {
            service: Some("zzz"),
            reply: (false, 6, OK),
            ..ORDINARY
        }),
        Entry::Card(Card {
            lower_description: Some(""),
            service: Some("bbb"),
            ..ORDINARY
        }),
        Entry::Card(Card {
            service: Some("aaa"),
            ..ORDINARY
        }),
    ]);
    let Observation::RegistryNdis(value) = scan(&source) else {
        panic!("expected first successful driver");
    };
    assert_eq!(value.official_ascii(), *b"02:0A:7B:FF:01:09");
    assert_eq!(
        *source.read.borrow(),
        [
            Read::Entry(0),
            Read::Entry(1),
            Read::Description(1),
            Read::Entry(2),
            Read::Description(2),
            Read::Entry(3),
            Read::Description(3),
            Read::Service(3),
            Read::Entry(4),
            Read::Description(4),
            Read::Service(4),
            Read::Ioctl(r"\\.\zzz".into()),
            Read::Entry(5),
            Read::Description(5),
            Read::Service(5),
            Read::Ioctl(r"\\.\bbb".into()),
        ]
    );
}

#[test]
fn outer_filter_rejects_first_nonempty_value_without_resuming_registry_scan() {
    for rejected in [[255; 6], [0, 0, 0, 0, 0, 42]] {
        let source = fixture(vec![
            Entry::Card(Card {
                service: Some("first"),
                reply: (true, 6, rejected),
                ..ORDINARY
            }),
            Entry::Card(ORDINARY),
        ]);
        assert!(matches!(
            scan(&source),
            Observation::FallbackRequired(Fallback::RejectedByOuterFilter)
        ));
        assert!(!source.read.borrow().contains(&Read::Entry(1)));
    }
    for accepted in [[1, 0, 0, 0, 0, 0], [0, 0, 0, 0, 1, 0]] {
        assert!(MacAddress(accepted).accepted());
    }
}

#[test]
fn exhausted_path_requests_only_the_unimplemented_fallback() {
    let source = fixture(vec![Entry::Card(Card {
        reply: (false, 0, OK),
        ..ORDINARY
    })]);
    assert!(matches!(
        scan(&source),
        Observation::FallbackRequired(Fallback::NoRegistryCandidate)
    ));
    assert_eq!(source.read.borrow().last(), Some(&Read::Entry(1)));
}

#[test]
fn bounded_scan_is_inconclusive_before_unexamined_entries() {
    let source = fixture((0..=MAX_CARDS).map(|_| Entry::Skip).collect());
    assert!(matches!(
        scan(&source),
        Observation::Inconclusive(Inconclusive::EnumerationLimit)
    ));
    assert_eq!(
        source.read.borrow().last(),
        Some(&Read::Entry(MAX_CARDS - 1))
    );
}

#[test]
fn a_short_success_or_malformed_registry_input_does_not_fabricate_a_candidate() {
    for (entry, expected) in [
        (Entry::Malformed, Inconclusive::RegistryKey),
        (
            Entry::Card(Card {
                lower_description: None,
                ..ORDINARY
            }),
            Inconclusive::Description,
        ),
        (
            Entry::Card(Card {
                service: Some("malformed-fixture"),
                ..ORDINARY
            }),
            Inconclusive::DeviceName,
        ),
        (
            Entry::Card(Card {
                service: Some(r"pipe\alternate"),
                ..ORDINARY
            }),
            Inconclusive::DeviceName,
        ),
        (
            Entry::Card(Card {
                service: Some("short"),
                reply: (true, 5, OK),
                ..ORDINARY
            }),
            Inconclusive::DriverReply,
        ),
        (
            Entry::Card(Card {
                service: Some("oversized"),
                reply: (true, 261, OK),
                ..ORDINARY
            }),
            Inconclusive::DriverReply,
        ),
    ] {
        let source = fixture(vec![entry, Entry::Card(ORDINARY)]);
        assert!(matches!(scan(&source), Observation::Inconclusive(reason) if reason == expected));
        assert!(!source.read.borrow().contains(&Read::Entry(1)));
    }
}

#[test]
fn bounded_utf16_registry_values_never_depend_on_uninitialized_data() {
    let mut buffer = [0u16; MAX_WIDE];
    buffer[..3].copy_from_slice(&[65, 66, 67]);
    assert_eq!(registry_units(&buffer, 6, false), Some(&buffer[..3]));
    assert_eq!(registry_units(&buffer, 6, true), None);
    assert_eq!(registry_units(&buffer, 8, true), Some(&buffer[..3]));
    assert_eq!(registry_units(&buffer, 7, false), None);
    assert_eq!(registry_units(&buffer, 522, true), None);
    assert_eq!(registry_units(&[65; MAX_WIDE], 520, false), None);
}

#[test]
fn collector_never_turns_a_registry_service_name_into_a_different_device_path() {
    for name in [
        "",
        ".",
        "..",
        "C:",
        "dir/adapter",
        "bad.",
        "bad ",
        "with\0nul",
    ] {
        assert!(device_path(&name.encode_utf16().collect::<Vec<_>>()).is_none());
    }
    assert!(device_path(&[65; 256]).is_none());
    assert_eq!(device_path(&[65; 255]).unwrap().len(), MAX_WIDE);
}
