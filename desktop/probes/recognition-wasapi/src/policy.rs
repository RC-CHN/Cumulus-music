use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

const WAVE_HEADER: usize = 18;
const WAVE_EXT: usize = 40;
pub(crate) const MAX_FRAMES_PER_PACKET: u32 = 48_000;
pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(10);

pub(crate) fn validate_request(frames: u32, deadline: Duration) -> Result<(), Fault> {
    if frames == 0 || frames > 192_000 || deadline.is_zero() || deadline > Duration::from_secs(8) {
        return Err(Fault::InvalidLimit);
    }
    Ok(())
}

pub(crate) fn next_wait(
    deadline: Duration,
    elapsed: Duration,
    cancel: &Cancellation,
) -> Result<Duration, Fault> {
    cancel.check()?;
    let remaining = deadline.saturating_sub(elapsed);
    if remaining.is_zero() {
        return Err(Fault::Timeout);
    }
    Ok(remaining.min(POLL_INTERVAL))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Fault {
    Endpoint,
    Format,
    Native(&'static str, i32),
    DeviceChanged,
    Cancelled,
    Timeout,
    InvalidLimit,
    InvalidPacket,
    Discontinuity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Sample {
    I16,
    F32,
}
impl Sample {
    fn bits(self) -> u16 {
        match self {
            Self::I16 => 16,
            Self::F32 => 32,
        }
    }
    fn tag(self) -> u16 {
        match self {
            Self::I16 => 1,
            Self::F32 => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Format {
    pub(crate) hz: u32,
    pub(crate) sample: Sample,
}
impl Format {
    pub(crate) fn frame_bytes(self) -> usize {
        usize::from(self.sample.bits() / 8) * 2
    }
}

// This ID is only a synthetic selector vector. There is no CLI, environment or
// default endpoint constructor. A real fixture must supply a trusted owner binding.
const SYNTHETIC_RENDER_ID: &str = "{0.0.0.00000000}.{bbcd8489-63ca-4fa6-b9ab-df503084c941}";

pub(crate) struct FixtureEndpoint(Vec<u16>);
impl FixtureEndpoint {
    #[cfg(test)]
    pub(crate) fn synthetic_vector() -> Self {
        Self(SYNTHETIC_RENDER_ID.encode_utf16().collect())
    }

    #[cfg(windows)]
    pub(crate) fn nul_terminated(&self) -> Vec<u16> {
        self.0.iter().copied().chain(Some(0)).collect()
    }

    pub(crate) fn id(&self) -> &[u16] {
        &self.0
    }

    pub(crate) fn verify(
        &self,
        observed_id: &[u16],
        active: bool,
        render: bool,
    ) -> Result<(), Fault> {
        if active && render && observed_id == self.id() {
            Ok(())
        } else {
            Err(Fault::Endpoint)
        }
    }
}

fn le16(bytes: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([bytes[off], bytes[off + 1]])
}
fn le32(bytes: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap())
}

const GUID_SUFFIX: [u8; 12] = [0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113];

pub(crate) fn parse_wave(bytes: &[u8]) -> Option<Format> {
    if bytes.len() < WAVE_HEADER || le16(bytes, 2) != 2 {
        return None;
    }
    let hz = le32(bytes, 4);
    if !matches!(hz, 44_100 | 48_000) {
        return None;
    }
    let bits = le16(bytes, 14);
    let tag = match (le16(bytes, 0), le16(bytes, 16), bytes.len()) {
        (1 | 3, 0, WAVE_HEADER) => le16(bytes, 0),
        (0xfffe, 22, WAVE_EXT)
            if le16(bytes, 18) == bits && le32(bytes, 20) == 3 && bytes[28..] == GUID_SUFFIX =>
        {
            u16::try_from(le32(bytes, 24)).ok()?
        }
        _ => return None,
    };
    let sample = match (tag, bits) {
        (1, 16) => Sample::I16,
        (3, 32) => Sample::F32,
        _ => return None,
    };
    let frame = (sample.bits() / 8) * 2;
    if le16(bytes, 12) != frame || le32(bytes, 8) != hz * u32::from(frame) {
        return None;
    }
    Some(Format { hz, sample })
}

fn ext_wave(format: Format) -> [u8; WAVE_EXT] {
    let mut raw = [0; WAVE_EXT];
    let frame = format.frame_bytes() as u16;
    raw[0..2].copy_from_slice(&0xfffe_u16.to_le_bytes());
    raw[2..4].copy_from_slice(&2_u16.to_le_bytes());
    raw[4..8].copy_from_slice(&format.hz.to_le_bytes());
    raw[8..12].copy_from_slice(&(format.hz * u32::from(frame)).to_le_bytes());
    raw[12..14].copy_from_slice(&frame.to_le_bytes());
    raw[14..16].copy_from_slice(&format.sample.bits().to_le_bytes());
    raw[16..18].copy_from_slice(&22_u16.to_le_bytes());
    raw[18..20].copy_from_slice(&format.sample.bits().to_le_bytes());
    raw[20..24].copy_from_slice(&3_u32.to_le_bytes());
    raw[24..28].copy_from_slice(&u32::from(format.sample.tag()).to_le_bytes());
    raw[28..].copy_from_slice(&GUID_SUFFIX);
    raw
}

pub(crate) fn negotiate<T>(
    mix: &[u8],
    mut init: impl FnMut(&[u8]) -> Result<T, Fault>,
) -> Result<(Format, T), Fault> {
    if let Some(format) = parse_wave(mix) {
        match init(mix) {
            Ok(client) => return Ok((format, client)),
            Err(Fault::Format) => {}
            Err(fault) => return Err(fault),
        }
    }
    // 30BFC0's 24 candidates, filtered to the four verified resampler input profiles.
    for sample in [Sample::I16, Sample::F32] {
        for hz in [48_000, 44_100] {
            let format = Format { hz, sample };
            let raw = ext_wave(format);
            if raw == mix {
                continue;
            }
            match init(&raw) {
                Ok(client) => return Ok((format, client)),
                Err(Fault::Format) => {}
                Err(fault) => return Err(fault),
            }
        }
    }
    Err(Fault::Format)
}

#[derive(Default)]
pub(crate) struct Cancellation(AtomicBool);
impl Cancellation {
    #[cfg(test)]
    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub(crate) fn check(&self) -> Result<(), Fault> {
        if self.0.load(Ordering::Acquire) {
            Err(Fault::Cancelled)
        } else {
            Ok(())
        }
    }
}

pub(crate) struct Budget {
    deadline: Duration,
    remaining: u32,
}
impl Budget {
    pub(crate) fn new(format: Format, frames: u32, deadline: Duration) -> Result<Self, Fault> {
        validate_request(frames, deadline)?;
        if frames > format.hz * 4 {
            return Err(Fault::InvalidLimit);
        }
        Ok(Self {
            deadline,
            remaining: frames,
        })
    }

    pub(crate) fn next_wait(
        &self,
        elapsed: Duration,
        cancel: &Cancellation,
    ) -> Result<Duration, Fault> {
        next_wait(self.deadline, elapsed, cancel)
    }

    pub(crate) fn complete(&self) -> bool {
        self.remaining == 0
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PacketInfo {
    pub(crate) frames: u32,
    pub(crate) flags: u32,
}
const DISCONTINUITY: u32 = 1;
pub(crate) const SILENT: u32 = 2;
const TIMESTAMP_ERROR: u32 = 4;

#[derive(Debug, PartialEq)]
pub(crate) enum Stereo {
    I16(Vec<[i16; 2]>),
    F32(Vec<[f32; 2]>),
}

pub(crate) trait Packet {
    fn info(&self) -> PacketInfo;
    fn bytes(&self, length: usize) -> Option<&[u8]>;
    fn release(self) -> Result<(), Fault>;
}

pub(crate) fn read_packet(
    packet: impl Packet,
    announced_frames: u32,
    format: Format,
    budget: &mut Budget,
    cancel: &Cancellation,
) -> Result<Stereo, Fault> {
    let decode = || {
        cancel.check()?;
        let PacketInfo { frames, flags } = packet.info();
        if flags & (DISCONTINUITY | TIMESTAMP_ERROR) != 0 {
            return Err(Fault::Discontinuity);
        }
        if flags & !SILENT != 0
            || frames == 0
            || frames != announced_frames
            || frames > MAX_FRAMES_PER_PACKET
            || budget.complete()
        {
            return Err(Fault::InvalidPacket);
        }
        let count = frames.min(budget.remaining) as usize;
        if flags == SILENT {
            return Ok((
                count,
                match format.sample {
                    Sample::I16 => Stereo::I16(vec![[0; 2]; count]),
                    Sample::F32 => Stereo::F32(vec![[0.0; 2]; count]),
                },
            ));
        }
        let length = count * format.frame_bytes();
        let bytes = packet
            .bytes(length)
            .filter(|b| b.len() == length)
            .ok_or(Fault::InvalidPacket)?;
        let stereo = match format.sample {
            Sample::I16 => Stereo::I16(
                bytes
                    .chunks_exact(4)
                    .map(|frame| {
                        [
                            i16::from_le_bytes(frame[..2].try_into().unwrap()),
                            i16::from_le_bytes(frame[2..].try_into().unwrap()),
                        ]
                    })
                    .collect(),
            ),
            Sample::F32 => {
                let values: Vec<[f32; 2]> = bytes
                    .chunks_exact(8)
                    .map(|frame| {
                        [
                            f32::from_le_bytes(frame[..4].try_into().unwrap()),
                            f32::from_le_bytes(frame[4..].try_into().unwrap()),
                        ]
                    })
                    .collect();
                if values
                    .iter()
                    .flatten()
                    .any(|value| !value.is_finite() || value.abs() > 4.0)
                {
                    return Err(Fault::InvalidPacket);
                }
                Stereo::F32(values)
            }
        };
        Ok((count, stereo))
    };
    let decoded = decode();
    packet.release()?;
    cancel.check()?;
    let (count, stereo) = decoded?;
    budget.remaining -= count as u32;
    Ok(stereo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const PCM_48_PLAIN: [u8; 18] = [
        0x01, 0x00, 0x02, 0x00, 0x80, 0xbb, 0x00, 0x00, 0x00, 0xee, 0x02, 0x00, 0x04, 0x00, 0x10,
        0x00, 0x00, 0x00,
    ];
    const FLOAT_44_EXT: [u8; 40] = [
        0xfe, 0xff, 0x02, 0x00, 0x44, 0xac, 0x00, 0x00, 0x20, 0x62, 0x05, 0x00, 0x08, 0x00, 0x20,
        0x00, 0x16, 0x00, 0x20, 0x00, 0x03, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
    ];
    const I16_48: Format = Format {
        hz: 48_000,
        sample: Sample::I16,
    };
    const F32_44: Format = Format {
        hz: 44_100,
        sample: Sample::F32,
    };

    #[test]
    fn explicit_fixture_id_is_exact_and_render_only() {
        let chosen = FixtureEndpoint::synthetic_vector();
        let wanted = chosen.id();
        assert_eq!(chosen.verify(wanted, true, true), Ok(()));
        for wrong in [
            vec![],
            "default".encode_utf16().collect(),
            wanted[1..].to_vec(),
            wanted.iter().copied().chain(Some(0)).collect(),
            SYNTHETIC_RENDER_ID.to_uppercase().encode_utf16().collect(),
        ] {
            assert_eq!(chosen.verify(&wrong, true, true), Err(Fault::Endpoint));
        }
        assert_eq!(chosen.verify(wanted, false, true), Err(Fault::Endpoint));
        assert_eq!(chosen.verify(wanted, true, false), Err(Fault::Endpoint));
    }

    #[test]
    fn frozen_wave_profiles_and_malformed_fields() {
        assert_eq!(parse_wave(&PCM_48_PLAIN), Some(I16_48));
        assert_eq!(parse_wave(&FLOAT_44_EXT), Some(F32_44));
        assert_eq!(ext_wave(F32_44), FLOAT_44_EXT);
        for &(idx, bad) in &[
            (2, 1),
            (4, 0),
            (8, 0),
            (12, 4),
            (14, 24),
            (16, 23),
            (18, 24),
            (20, 4),
            (24, 1),
            (27, 1),
            (33, 1),
        ] {
            let mut malformed = FLOAT_44_EXT;
            malformed[idx] = bad;
            assert_eq!(parse_wave(&malformed), None, "changed byte {idx}");
        }
        for n in 0..FLOAT_44_EXT.len() {
            assert_eq!(parse_wave(&FLOAT_44_EXT[..n]), None, "length {n}");
        }
        assert_eq!(parse_wave(&[FLOAT_44_EXT.as_slice(), &[0]].concat()), None);
    }

    #[test]
    fn negotiation_attempts_exact_first_and_filters_native_order() {
        let mut calls = Vec::new();
        let (_, receipt) = negotiate(&PCM_48_PLAIN, |raw| {
            calls.push(raw.to_vec());
            Ok(9)
        })
        .unwrap();
        assert_eq!(receipt, 9);
        assert_eq!(calls, [PCM_48_PLAIN.to_vec()]);

        let mut calls = Vec::new();
        assert_eq!(
            negotiate::<()>(&[], |raw| {
                calls.push(parse_wave(raw).unwrap());
                Err(Fault::Format)
            }),
            Err(Fault::Format)
        );
        assert_eq!(
            calls,
            [
                I16_48,
                Format {
                    hz: 44_100,
                    sample: Sample::I16
                },
                Format {
                    hz: 48_000,
                    sample: Sample::F32
                },
                F32_44,
            ]
        );

        calls.clear();
        assert_eq!(
            negotiate(&FLOAT_44_EXT, |raw| {
                let format = parse_wave(raw).unwrap();
                calls.push(format);
                if format == I16_48 {
                    Ok(4)
                } else {
                    Err(Fault::Format)
                }
            }),
            Ok((I16_48, 4))
        );
        assert_eq!(calls, [F32_44, I16_48]);

        for terminal in [
            Fault::Native("Initialize", -1),
            Fault::DeviceChanged,
            Fault::Cancelled,
        ] {
            let count = Cell::new(0);
            assert_eq!(
                negotiate::<()>(&PCM_48_PLAIN, |_| {
                    count.set(count.get() + 1);
                    Err(terminal)
                }),
                Err(terminal)
            );
            assert_eq!(count.get(), 1);
        }
    }

    struct FakePacket<'a> {
        meta: PacketInfo,
        data: Option<&'a [u8]>,
        accessed: &'a Cell<usize>,
        released: &'a Cell<u32>,
        fail_release: bool,
        cancel_on_release: Option<&'a Cancellation>,
    }
    impl Packet for FakePacket<'_> {
        fn info(&self) -> PacketInfo {
            self.meta
        }
        fn bytes(&self, n: usize) -> Option<&[u8]> {
            self.accessed.set(self.accessed.get() + 1);
            self.data?.get(..n)
        }
        fn release(self) -> Result<(), Fault> {
            assert_eq!(self.released.get(), 0, "double release");
            self.released.set(self.meta.frames);
            if let Some(token) = self.cancel_on_release {
                token.cancel();
            }
            if self.fail_release {
                return Err(Fault::Native("ReleaseBuffer", -1));
            }
            Ok(())
        }
    }

    fn packet<'a>(
        frames: u32,
        flags: u32,
        data: Option<&'a [u8]>,
        accessed: &'a Cell<usize>,
        released: &'a Cell<u32>,
    ) -> FakePacket<'a> {
        FakePacket {
            meta: PacketInfo { frames, flags },
            data,
            accessed,
            released,
            fail_release: false,
            cancel_on_release: None,
        }
    }

    #[test]
    fn packets_are_bounded_stereo_and_release_full_native_count() {
        let cancel = Cancellation::default();
        let accessed = Cell::new(0);
        let released = Cell::new(0);
        let mut budget = Budget::new(I16_48, 2, Duration::from_secs(1)).unwrap();
        let input = [0x00, 0x80, 0xff, 0x7f, 0x34, 0x12, 0xfe, 0xff];
        let acquired = packet(3, 0, Some(&input), &accessed, &released);
        assert_eq!(
            read_packet(acquired, 3, I16_48, &mut budget, &cancel),
            Ok(Stereo::I16(vec![[-32768, 32767], [4660, -2]]))
        );
        assert_eq!(released.get(), 3);
        assert_eq!(accessed.get(), 1);
        assert!(budget.complete());

        accessed.set(0);
        released.set(0);
        let mut budget = Budget::new(F32_44, 1, Duration::from_secs(1)).unwrap();
        let input = [0, 0, 128, 63, 0, 0, 0, 191];
        assert_eq!(
            read_packet(
                packet(1, 0, Some(&input), &accessed, &released),
                1,
                F32_44,
                &mut budget,
                &cancel
            ),
            Ok(Stereo::F32(vec![[1.0, -0.5]]))
        );
        assert_eq!(released.get(), 1);
        assert!(budget.complete());

        accessed.set(0);
        released.set(0);
        let mut budget = Budget::new(F32_44, 1, Duration::from_secs(1)).unwrap();
        let silent = packet(1, SILENT, None, &accessed, &released);
        assert_eq!(
            read_packet(silent, 1, F32_44, &mut budget, &cancel),
            Ok(Stereo::F32(vec![[0.0; 2]]))
        );
        assert_eq!(accessed.get(), 0);
        assert_eq!(released.get(), 1);
    }

    #[test]
    fn rejection_still_releases_without_advancing_the_budget() {
        let nan = [0, 0, 192, 127, 0, 0, 0, 0];
        let excessive_float = [0, 0, 160, 64, 0, 0, 0, 0];
        for &(frames, announced, flags, data, accessed_count, want) in &[
            (2, 2, DISCONTINUITY | SILENT, None, 0, Fault::Discontinuity),
            (2, 2, TIMESTAMP_ERROR, None, 0, Fault::Discontinuity),
            (2, 2, 8, None, 0, Fault::InvalidPacket),
            (2, 3, 0, None, 0, Fault::InvalidPacket),
            (48_001, 48_001, SILENT, None, 0, Fault::InvalidPacket),
            (2, 2, 0, None, 1, Fault::InvalidPacket),
            (2, 2, 0, Some(nan.as_slice()), 1, Fault::InvalidPacket),
            (
                2,
                2,
                0,
                Some(excessive_float.as_slice()),
                1,
                Fault::InvalidPacket,
            ),
        ] {
            let accessed = Cell::new(0);
            let released = Cell::new(0);
            let cancel = Cancellation::default();
            let mut budget = Budget::new(F32_44, 1, Duration::from_secs(1)).unwrap();
            let acquired = packet(frames, flags, data, &accessed, &released);
            assert_eq!(
                read_packet(acquired, announced, F32_44, &mut budget, &cancel),
                Err(want)
            );
            assert_eq!(accessed.get(), accessed_count);
            assert_eq!(released.get(), frames);
            assert!(!budget.complete());
        }
    }

    #[test]
    fn cancellation_release_failure_and_deadline_do_not_deliver_samples() {
        let timeout = Duration::from_millis(42);
        let mut budget = Budget::new(I16_48, 2, timeout).unwrap();
        let cancel = Cancellation::default();
        assert_eq!(
            budget.next_wait(Duration::ZERO, &cancel),
            Ok(Duration::from_millis(10))
        );
        assert_eq!(
            budget.next_wait(Duration::from_millis(41), &cancel),
            Ok(Duration::from_millis(1))
        );
        assert_eq!(budget.next_wait(timeout, &cancel), Err(Fault::Timeout));
        assert!(matches!(
            Budget::new(I16_48, 192_001, timeout),
            Err(Fault::InvalidLimit)
        ));
        assert!(matches!(
            Budget::new(I16_48, 1, Duration::from_secs(9)),
            Err(Fault::InvalidLimit)
        ));

        let accessed = Cell::new(0);
        let released = Cell::new(0);
        let mut acquired = packet(2, SILENT, None, &accessed, &released);
        acquired.fail_release = true;
        assert_eq!(
            read_packet(acquired, 2, I16_48, &mut budget, &cancel),
            Err(Fault::Native("ReleaseBuffer", -1))
        );
        assert_eq!(released.get(), 2);
        assert!(!budget.complete());

        released.set(0);
        let mut acquired = packet(2, SILENT, None, &accessed, &released);
        acquired.cancel_on_release = Some(&cancel);
        assert_eq!(
            read_packet(acquired, 2, I16_48, &mut budget, &cancel),
            Err(Fault::Cancelled)
        );
        assert_eq!(released.get(), 2);
        assert!(!budget.complete());
        assert_eq!(
            budget.next_wait(Duration::ZERO, &cancel),
            Err(Fault::Cancelled)
        );

        released.set(0);
        let acquired = packet(2, 0, None, &accessed, &released);
        assert_eq!(
            read_packet(acquired, 2, I16_48, &mut budget, &cancel),
            Err(Fault::Cancelled)
        );
        assert_eq!(released.get(), 2);
        assert_eq!(accessed.get(), 0);
    }
}
