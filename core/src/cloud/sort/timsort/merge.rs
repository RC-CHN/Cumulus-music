use super::{Sort, gallop};
use std::cmp::Ordering;
impl<F: Fn(usize, usize) -> Ordering> Sort<F> {
    pub(super) fn merge_low(&mut self, a: usize, mut na: usize, b: usize, mut nb: usize) {
        let temp = self.values[a..a + na].to_vec();
        let (mut out, mut ca, mut cb) = (a, 0, b);
        self.values[out] = self.values[cb];
        out += 1;
        cb += 1;
        nb -= 1;
        let mut min = self.min_gallop;
        'merge: while na > 1 && nb > 0 {
            let (mut wa, mut wb) = (0, 0);
            loop {
                if (self.compare)(self.values[cb], temp[ca]) == Ordering::Less {
                    self.values[out] = self.values[cb];
                    out += 1;
                    cb += 1;
                    nb -= 1;
                    wb += 1;
                    wa = 0;
                    if nb == 0 {
                        break 'merge;
                    }
                    if wb >= min {
                        break;
                    }
                } else {
                    self.values[out] = temp[ca];
                    out += 1;
                    ca += 1;
                    na -= 1;
                    wa += 1;
                    wb = 0;
                    if na == 1 {
                        break 'merge;
                    }
                    if wa >= min {
                        break;
                    }
                }
            }
            min += 1;
            let mut first = true;
            while first || wa >= 7 || wb >= 7 {
                first = false;
                min = (min - 1).max(1);
                self.min_gallop = min;
                wa = gallop::right(&temp, self.values[cb], ca, na, 0, &self.compare);
                if wa > 0 {
                    self.values[out..out + wa].copy_from_slice(&temp[ca..ca + wa]);
                    out += wa;
                    ca += wa;
                    na -= wa;
                    if na <= 1 {
                        break 'merge;
                    }
                }
                self.values[out] = self.values[cb];
                out += 1;
                cb += 1;
                nb -= 1;
                if nb == 0 {
                    break 'merge;
                }
                wb = gallop::left(&self.values, temp[ca], cb, nb, 0, &self.compare);
                if wb > 0 {
                    self.values.copy_within(cb..cb + wb, out);
                    out += wb;
                    cb += wb;
                    nb -= wb;
                    if nb == 0 {
                        break 'merge;
                    }
                }
                self.values[out] = temp[ca];
                out += 1;
                ca += 1;
                na -= 1;
                if na == 1 {
                    break 'merge;
                }
            }
            min += 1;
            self.min_gallop = min;
        }
        if na == 1 && nb > 0 {
            self.values.copy_within(cb..cb + nb, out);
            self.values[out + nb] = temp[ca];
        } else if na > 0 {
            self.values[out..out + na].copy_from_slice(&temp[ca..ca + na]);
        }
    }
    pub(super) fn merge_high(&mut self, a: usize, mut na: usize, b: usize, mut nb: usize) {
        let temp = self.values[b..b + nb].to_vec();
        // End-exclusive cursors avoid a -1/usize underflow at the left edge.
        let (mut out, mut ca, mut cb) = (b + nb, a + na, nb);
        out -= 1;
        ca -= 1;
        self.values[out] = self.values[ca];
        na -= 1;
        let mut min = self.min_gallop;
        'merge: while na > 0 && nb > 1 {
            let (mut wa, mut wb) = (0, 0);
            loop {
                if (self.compare)(temp[cb - 1], self.values[ca - 1]) == Ordering::Less {
                    out -= 1;
                    ca -= 1;
                    self.values[out] = self.values[ca];
                    na -= 1;
                    wa += 1;
                    wb = 0;
                    if na == 0 {
                        break 'merge;
                    }
                    if wa >= min {
                        break;
                    }
                } else {
                    out -= 1;
                    cb -= 1;
                    self.values[out] = temp[cb];
                    nb -= 1;
                    wb += 1;
                    wa = 0;
                    if nb == 1 {
                        break 'merge;
                    }
                    if wb >= min {
                        break;
                    }
                }
            }
            min += 1;
            let mut first = true;
            while first || wa >= 7 || wb >= 7 {
                first = false;
                min = (min - 1).max(1);
                self.min_gallop = min;
                wa = na - gallop::right(&self.values, temp[cb - 1], a, na, na - 1, &self.compare);
                if wa > 0 {
                    out -= wa;
                    ca -= wa;
                    self.values.copy_within(ca..ca + wa, out);
                    na -= wa;
                    if na == 0 {
                        break 'merge;
                    }
                }
                out -= 1;
                cb -= 1;
                self.values[out] = temp[cb];
                nb -= 1;
                if nb == 1 {
                    break 'merge;
                }
                wb = nb - gallop::left(&temp, self.values[ca - 1], 0, nb, nb - 1, &self.compare);
                if wb > 0 {
                    out -= wb;
                    cb -= wb;
                    self.values[out..out + wb].copy_from_slice(&temp[cb..cb + wb]);
                    nb -= wb;
                    if nb <= 1 {
                        break 'merge;
                    }
                }
                out -= 1;
                ca -= 1;
                self.values[out] = self.values[ca];
                na -= 1;
                if na == 0 {
                    break 'merge;
                }
            }
            min += 1;
            self.min_gallop = min;
        }
        if nb == 1 && na > 0 {
            out -= na;
            ca -= na;
            self.values.copy_within(ca..ca + na, out);
            self.values[out - 1] = temp[cb - 1];
        } else if nb > 0 {
            self.values[out - nb..out].copy_from_slice(&temp[..nb]);
        }
    }
}
