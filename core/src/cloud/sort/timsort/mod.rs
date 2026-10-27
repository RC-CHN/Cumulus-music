//! Safe index-only port of V8 13.6 ArrayTimSortImpl. M/177 is not transitive
//! for all mixed strings, so the sequence of comparisons is observable.
//! See core/third-party/V8-sort-NOTICE.txt for provenance and licensing.
mod gallop;
mod merge;
use std::cmp::Ordering;

/// V8-style stable ordering without assuming the comparator is transitive.
/// Returned indices are a permutation of `0..len`; inputs never move or clone.
pub fn stable_order(len: usize, compare: impl Fn(usize, usize) -> Ordering) -> Vec<usize> {
    let mut state = Sort {
        values: (0..len).collect(),
        runs: vec![],
        min_gallop: 7,
        compare,
    };
    if len < 2 {
        return state.values;
    }
    let mut n = len;
    let mut bits = 0;
    while n >= 64 {
        bits |= n & 1;
        n >>= 1;
    }
    let min_run = n + bits;
    let mut base = 0;
    while base < len {
        let mut run = state.run(base);
        if run < min_run {
            let forced = min_run.min(len - base);
            state.insertion(base, base + run, base + forced);
            run = forced;
        }
        state.runs.push((base, run));
        state.collapse(false);
        base += run;
    }
    state.collapse(true);
    state.values
}
struct Sort<F> {
    values: Vec<usize>,
    runs: Vec<(usize, usize)>,
    min_gallop: usize,
    compare: F,
}
impl<F: Fn(usize, usize) -> Ordering> Sort<F> {
    fn run(&mut self, base: usize) -> usize {
        if base + 1 == self.values.len() {
            return 1;
        }
        let less = |a, b| (self.compare)(a, b) == Ordering::Less;
        let descending = less(self.values[base + 1], self.values[base]);
        let mut end = base + 2;
        while end < self.values.len() && less(self.values[end], self.values[end - 1]) == descending
        {
            end += 1;
        }
        if descending {
            self.values[base..end].reverse();
        }
        end - base
    }
    fn insertion(&mut self, base: usize, start: usize, end: usize) {
        for position in start.max(base + 1)..end {
            let value = self.values[position];
            let (mut low, mut high) = (base, position);
            while low < high {
                let mid = low + (high - low) / 2;
                if (self.compare)(value, self.values[mid]) == Ordering::Less {
                    high = mid;
                } else {
                    low = mid + 1;
                }
            }
            self.values.copy_within(low..position, low + 1);
            self.values[low] = value;
        }
    }
    fn invariant(&self, index: usize) -> bool {
        index < 2 || self.runs[index - 2].1 > self.runs[index - 1].1 + self.runs[index].1
    }
    fn collapse(&mut self, force: bool) {
        while self.runs.len() > 1 {
            let mut index = self.runs.len() - 2;
            if force {
                if index > 0 && self.runs[index - 1].1 < self.runs[index + 1].1 {
                    index -= 1;
                }
            } else if !self.invariant(index + 1) || !self.invariant(index) {
                if self.runs[index - 1].1 < self.runs[index + 1].1 {
                    index -= 1;
                }
            } else if self.runs[index].1 > self.runs[index + 1].1 {
                break;
            }
            self.merge_at(index);
        }
    }
    fn merge_at(&mut self, index: usize) {
        let (mut a, mut na) = self.runs[index];
        let (b, mut nb) = self.runs.remove(index + 1);
        self.runs[index].1 = na + nb;
        let skip = gallop::right(&self.values, self.values[b], a, na, 0, &self.compare);
        a += skip;
        na -= skip;
        if na == 0 {
            return;
        }
        nb = gallop::left(
            &self.values,
            self.values[a + na - 1],
            b,
            nb,
            nb - 1,
            &self.compare,
        );
        if nb == 0 {
            return;
        }
        if na <= nb {
            self.merge_low(a, na, b, nb);
        } else {
            self.merge_high(a, na, b, nb);
        }
    }
}
