#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    Board,
    Bios,
}

pub(super) struct Observed<T, C> {
    pub(super) board: (T, C),
    pub(super) bios: (T, Option<C>),
    pub(super) selected: Option<Source>,
}

/// CEEFF0 reads both first results before validating the board; it does
/// not validate BIOS unless board selection was empty.
pub(super) fn select<T, C>(
    mut read: impl FnMut(Source) -> T,
    mut check: impl FnMut(&T) -> C,
    accepted: impl Fn(&C) -> bool,
) -> Observed<T, C> {
    let board = read(Source::Board);
    let bios = read(Source::Bios);
    let board_check = check(&board);
    let (bios_check, selected) = if accepted(&board_check) {
        (None, Some(Source::Board))
    } else {
        let result = check(&bios);
        let selected = accepted(&result).then_some(Source::Bios);
        (Some(result), selected)
    };
    Observed {
        board: (board, board_check),
        bios: (bios, bios_check),
        selected,
    }
}

/// CF7680 ignores this return. The probe only accepts an error-122 buffer
/// if all preinitialized non-NUL units match the same-call full control.
pub(super) fn complete_short(
    full_content: &[u16],
    short_content: &[u16],
    returned: i32,
    last_error: u32,
) -> bool {
    returned == 0
        && last_error == 122
        && !full_content.is_empty()
        && !full_content.contains(&0)
        && short_content == full_content
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn both_first_objects_are_read_and_board_precedes_bios() {
        for (allowed, expected, check_bios) in [
            ([true, true], Some(Source::Board), false),
            ([false, true], Some(Source::Bios), true),
            ([false, false], None, true),
        ] {
            let trace = RefCell::new(Vec::new());
            let observed = select(
                |source| {
                    trace.borrow_mut().push((false, source));
                    source
                },
                |&source| {
                    trace.borrow_mut().push((true, source));
                    allowed[usize::from(source == Source::Bios)]
                },
                |&valid| valid,
            );
            let mut expected_trace = vec![
                (false, Source::Board),
                (false, Source::Bios),
                (true, Source::Board),
            ];
            if check_bios {
                expected_trace.push((true, Source::Bios));
            }
            assert_eq!(trace.into_inner(), expected_trace);
            assert_eq!(observed.board, (Source::Board, allowed[0]));
            assert_eq!(
                observed.bios,
                (Source::Bios, check_bios.then_some(allowed[1]))
            );
            assert_eq!(observed.selected, expected);
        }
    }

    #[test]
    fn short_decoder_only_admits_complete_matching_non_nul_content() {
        // A supplementary character occupies two units, not one.
        let expected = [65, 0xd801, 0xdc37, 90];
        assert!(complete_short(&expected, &expected, 0, 122));
        assert!(!complete_short(&expected, &expected[..3], 0, 122));
        assert!(!complete_short(&expected, &[65, 0xd801, 0xdc37, 0], 0, 122));
        assert!(!complete_short(&expected, &expected, 0, 87));
        assert!(!complete_short(&expected, &expected, 4, 0));
        assert!(!complete_short(&[65, 0], &[65, 0], 0, 122));
    }
}
