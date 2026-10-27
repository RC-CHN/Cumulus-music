//! M/177 comparison: ASCII group first, Chinese numeric collation, then
//! recursive UTF-16 suffix comparison for mixed strings. Data is bundled ICU4X.
use crate::{Error, Result, protocol::wire::serialization::is_js_whitespace};
use icu_collator::{Collator, CollatorBorrowed, CollatorPreferences};
use icu_locale_core::locale;
use std::cmp::Ordering;
mod timsort;
pub use timsort::stable_order;

pub struct CloudTextOrder(CollatorBorrowed<'static>);
impl CloudTextOrder {
    pub fn new() -> Result<Self> {
        let preferences: CollatorPreferences = locale!("zh-Hans-CN-u-kn-true").into();
        Collator::try_new(preferences, Default::default())
            .map(Self)
            .map_err(|_| Error::State("Chinese numeric collation data unavailable"))
    }
    pub fn compare(&self, left: &str, right: &str) -> Ordering {
        self.compare_utf16(
            &left.encode_utf16().collect::<Vec<_>>(),
            &right.encode_utf16().collect::<Vec<_>>(),
        )
    }
    pub(crate) fn compare_utf16(&self, mut left: &[u16], mut right: &[u16]) -> Ordering {
        loop {
            left = trim(left);
            right = trim(right);
            if left == right {
                return Ordering::Equal;
            }
            if left.is_empty() || right.is_empty() {
                return left.len().cmp(&right.len());
            }
            let ascii = |v: u16| v <= 127 && (v as u8).is_ascii_alphanumeric();
            if (left.iter().all(|v| ascii(*v)) && right.iter().all(|v| ascii(*v)))
                || (left.iter().all(|v| !ascii(*v)) && right.iter().all(|v| !ascii(*v)))
            {
                return self.collate(left, right);
            }
            match (ascii(left[0]), ascii(right[0])) {
                (true, false) => return Ordering::Less,
                (false, true) => return Ordering::Greater,
                _ => {}
            }
            let result = self.unit(left[0], right[0]);
            if result != Ordering::Equal {
                return result;
            }
            left = &left[1..];
            right = &right[1..];
        }
    }
    fn collate(&self, left: &[u16], right: &[u16]) -> Ordering {
        let low = |v: &[u16]| v.first().is_some_and(|c| (0xdc00..=0xdfff).contains(c));
        match (low(left), low(right)) {
            (true, true) => left[0]
                .cmp(&right[0])
                .then_with(|| self.0.compare_utf16(&left[1..], &right[1..])),
            (true, false) => match self.0.compare_utf16(&[0xe000], right) {
                Ordering::Equal => Ordering::Less,
                v => v,
            },
            (false, true) => self.collate(right, left).reverse(),
            _ => self.0.compare_utf16(left, right),
        }
    }
    fn unit(&self, left: u16, right: u16) -> Ordering {
        let surrogate = |v| (0xd800..=0xdfff).contains(&v);
        match (surrogate(left), surrogate(right)) {
            (true, true) => left.cmp(&right),
            // ICU4C orders unpaired UTF-16 surrogates by code unit immediately
            // before the private-use block. ICU4X substitutes U+FFFD instead.
            // Only M/177's single-unit recursion needs this compatibility path.
            (true, false) => match self.0.compare_utf16(&[0xe000], &[right]) {
                Ordering::Equal => Ordering::Less,
                v => v,
            },
            (false, true) => self.unit(right, left).reverse(),
            _ => self.0.compare_utf16(&[left], &[right]),
        }
    }
}
fn trim(mut value: &[u16]) -> &[u16] {
    let space = |v: &u16| char::from_u32(u32::from(*v)).is_some_and(is_js_whitespace);
    while value.first().is_some_and(space) {
        value = &value[1..];
    }
    while value.last().is_some_and(space) {
        value = &value[..value.len() - 1];
    }
    value
}
