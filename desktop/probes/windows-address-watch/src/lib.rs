//! Explicit IPv4 address-table observation; independent of the app and network access policy.
#![cfg(any(test, all(windows, target_arch = "x86_64")))]

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows;
#[cfg(all(windows, target_arch = "x86_64"))]
pub use windows::{AddressWatcher, NativeError};

use std::time::Duration;

const MAX_WAIT_MS: u32 = 500;
const CLOSE_WAIT_MS: u32 = 250;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Wake<E> {
    Idle,
    /// The previous notification completed. Rearming was attempted first.
    Changed {
        rearm: Result<(), E>,
    },
    /// The OS canceled the request; this is not an address observation.
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitError<E> {
    TimeoutTooLarge,
    NotArmed,
    Native(E),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Close {
    Completed,
    /// The kernel's terminal status could not be established. The sole
    /// process-wide slot and its storage stay alive until the process exits.
    Retained,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Completion {
    Idle,
    Changed,
    Cancelled,
}

trait Registration {
    type Error;

    fn arm(&mut self) -> Result<(), Self::Error>;
    fn wait(&mut self, milliseconds: u32) -> Result<Completion, Self::Error>;
    fn request_cancel(&mut self);
    fn retain(self);
}

struct Watch<R: Registration> {
    registration: Option<R>,
    pending: bool,
}

impl<R: Registration> Watch<R> {
    fn start(mut registration: R) -> Result<Self, R::Error> {
        registration.arm()?;
        Ok(Self {
            registration: Some(registration),
            pending: true,
        })
    }

    fn rearm(&mut self) -> Result<(), R::Error> {
        if !self.pending {
            self.registration.as_mut().expect("open watcher").arm()?;
            self.pending = true;
        }
        Ok(())
    }

    fn wait(&mut self, timeout: Duration) -> Result<Wake<R::Error>, WaitError<R::Error>> {
        if timeout > Duration::from_millis(u64::from(MAX_WAIT_MS)) {
            return Err(WaitError::TimeoutTooLarge);
        }
        if !self.pending {
            return Err(WaitError::NotArmed);
        }
        match self
            .registration
            .as_mut()
            .expect("open watcher")
            .wait(timeout.as_millis() as u32)
            .map_err(WaitError::Native)?
        {
            Completion::Idle => Ok(Wake::Idle),
            Completion::Cancelled => {
                self.pending = false;
                Ok(Wake::Cancelled)
            }
            Completion::Changed => {
                self.pending = false;
                Ok(Wake::Changed {
                    rearm: self.rearm(),
                })
            }
        }
    }

    fn close(&mut self) -> Close {
        let Some(mut registration) = self.registration.take() else {
            return Close::Completed;
        };
        if !self.pending {
            return Close::Completed;
        }
        self.pending = false;
        registration.request_cancel();
        match registration.wait(CLOSE_WAIT_MS) {
            Ok(Completion::Changed | Completion::Cancelled) => Close::Completed,
            Ok(Completion::Idle) | Err(_) => {
                registration.retain();
                Close::Retained
            }
        }
    }
}

impl<R: Registration> Drop for Watch<R> {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::{Close, Completion, Registration, WaitError, Wake, Watch};
    use std::{cell::RefCell, collections::VecDeque, rc::Rc, time::Duration};

    type Log = Rc<RefCell<Vec<Action>>>;

    #[derive(Debug, PartialEq, Eq)]
    enum Action {
        Arm,
        Wait(u32),
        Cancel,
        Release,
        Retain,
    }

    struct Fake {
        arms: VecDeque<Result<(), &'static str>>,
        waits: VecDeque<Result<Completion, &'static str>>,
        log: Log,
        retained: bool,
    }

    impl Fake {
        fn setup(
            arms: impl IntoIterator<Item = Result<(), &'static str>>,
            waits: impl IntoIterator<Item = Result<Completion, &'static str>>,
        ) -> (Self, Log) {
            let log = Log::default();
            (
                Self {
                    arms: arms.into_iter().collect(),
                    waits: waits.into_iter().collect(),
                    log: log.clone(),
                    retained: false,
                },
                log,
            )
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            if !self.retained {
                self.log.borrow_mut().push(Action::Release);
            }
        }
    }

    impl Registration for Fake {
        type Error = &'static str;
        fn arm(&mut self) -> Result<(), Self::Error> {
            self.log.borrow_mut().push(Action::Arm);
            self.arms.pop_front().unwrap()
        }
        fn wait(&mut self, milliseconds: u32) -> Result<Completion, Self::Error> {
            self.log.borrow_mut().push(Action::Wait(milliseconds));
            self.waits.pop_front().unwrap()
        }
        fn request_cancel(&mut self) {
            self.log.borrow_mut().push(Action::Cancel);
        }
        fn retain(mut self) {
            self.retained = true;
            self.log.borrow_mut().push(Action::Retain);
        }
    }

    #[test]
    fn change_is_exposed_only_after_the_next_arm_attempt() {
        use Action::*;
        let (io, log) = Fake::setup(
            [Ok(()), Err("temporarily unavailable"), Ok(())],
            [Ok(Completion::Changed), Ok(Completion::Cancelled)],
        );
        let mut watch = Watch::start(io).unwrap();
        assert_eq!(
            watch.wait(Duration::from_millis(500)),
            Ok(Wake::Changed {
                rearm: Err("temporarily unavailable")
            })
        );
        assert_eq!(*log.borrow(), [Arm, Wait(500), Arm]);
        assert_eq!(watch.wait(Duration::ZERO), Err(WaitError::NotArmed));
        watch.rearm().unwrap();
        assert_eq!(watch.wait(Duration::ZERO), Ok(Wake::Cancelled));
        assert_eq!(watch.close(), Close::Completed);
        assert_eq!(*log.borrow(), [Arm, Wait(500), Arm, Arm, Wait(0), Release]);
    }

    #[test]
    fn cancellation_completion_releases_once_and_a_racing_change_is_not_published() {
        use Action::*;
        for completion in [Completion::Changed, Completion::Cancelled] {
            let (io, log) = Fake::setup([Ok(())], [Ok(completion)]);
            let mut watch = Watch::start(io).unwrap();
            assert_eq!(
                watch.wait(Duration::from_millis(501)),
                Err(WaitError::TimeoutTooLarge)
            );
            assert_eq!(watch.close(), Close::Completed);
            drop(watch);
            assert_eq!(*log.borrow(), [Arm, Cancel, Wait(250), Release]);
        }
    }

    #[test]
    fn unacknowledged_cancellation_never_frees_registration_storage() {
        use Action::*;
        for wait in [Ok(Completion::Idle), Err("invalid or unconfirmed handle")] {
            let (io, log) = Fake::setup([Ok(())], [wait]);
            let mut watch = Watch::start(io).unwrap();
            assert_eq!(watch.close(), Close::Retained);
            drop(watch);
            assert_eq!(*log.borrow(), [Arm, Cancel, Wait(250), Retain]);
        }
    }

    #[test]
    fn failed_registration_has_no_pending_storage_to_cancel() {
        let (io, log) = Fake::setup([Err("no registration")], []);
        assert!(Watch::start(io).is_err());
        assert_eq!(*log.borrow(), [Action::Arm, Action::Release]);
    }
}
