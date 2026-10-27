use std::cmp::Ordering;
pub(super) fn left(
    v: &[usize],
    key: usize,
    base: usize,
    len: usize,
    hint: usize,
    compare: &impl Fn(usize, usize) -> Ordering,
) -> usize {
    search(v, base, len, hint, |index| {
        compare(v[index], key) == Ordering::Less
    })
}
pub(super) fn right(
    v: &[usize],
    key: usize,
    base: usize,
    len: usize,
    hint: usize,
    compare: &impl Fn(usize, usize) -> Ordering,
) -> usize {
    search(v, base, len, hint, |index| {
        compare(key, v[index]) != Ordering::Less
    })
}
fn search(
    v: &[usize],
    base: usize,
    len: usize,
    hint: usize,
    preceding: impl Fn(usize) -> bool,
) -> usize {
    debug_assert!(len > 0 && hint < len && base + len <= v.len());
    let (mut last, mut offset) = (0isize, 1isize);
    let h = hint as isize;
    if preceding(base + hint) {
        let max = (len - hint) as isize;
        while offset < max && preceding(base + hint + offset as usize) {
            last = offset;
            offset = offset.saturating_mul(2).saturating_add(1);
        }
        offset = offset.min(max);
        last += h;
        offset += h;
    } else {
        let max = h + 1;
        while offset < max && !preceding(base + hint - offset as usize) {
            last = offset;
            offset = offset.saturating_mul(2).saturating_add(1);
        }
        offset = offset.min(max);
        (last, offset) = (h - offset, h - last);
    }
    last += 1;
    while last < offset {
        let middle = last + (offset - last) / 2;
        if preceding(base + middle as usize) {
            last = middle + 1;
        } else {
            offset = middle;
        }
    }
    offset as usize
}
