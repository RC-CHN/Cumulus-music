//! Strict diagnostics for SAFEARRAY metadata; no chassis meaning is inferred.
pub(super) const ARRAY_I4: u16 = 0x2003;
pub(super) const ARRAY_UI2: u16 = 0x2012;

pub(super) fn integer_array(variant: u16) -> Option<(u16, u32)> {
    match variant {
        ARRAY_I4 => Some((3, 4)),
        ARRAY_UI2 => Some((18, 2)),
        _ => None,
    }
}

pub(super) fn bounded_rank_one(lower: i32, upper: i32) -> bool {
    // OLE bounds are signed. Subtract in a wider type; negative lower
    // bounds are legal, but reversed or excessive spans aren't useful here.
    (1..=4_096).contains(&(i64::from(upper) - i64::from(lower) + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_pointer_access_for_byref_variant_and_scalar_tags() {
        assert_eq!(integer_array(0x2003), Some((3, 4)));
        assert_eq!(integer_array(0x2012), Some((18, 2)));
        for tag in [0, 1, 3, 18, 0x6003, 0x6012, 0x2008, 0x200c, 0xffff] {
            assert_eq!(integer_array(tag), None, "tag 0x{tag:04x}");
        }
    }

    #[test]
    fn signed_bounds_do_not_wrap_or_require_zero_lower_bound() {
        assert!(bounded_rank_one(-5, -5));
        assert!(bounded_rank_one(-1, 4_094));
        assert!(!bounded_rank_one(-1, 4_095));
        assert!(!bounded_rank_one(7, 6));
        assert!(!bounded_rank_one(i32::MIN, i32::MAX));
        assert!(bounded_rank_one(i32::MAX, i32::MAX));
    }
}
