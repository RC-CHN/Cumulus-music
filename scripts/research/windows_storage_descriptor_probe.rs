//! Explicit offline observation of two read-only IOCTLs from the recovered
//! Windows x64 CF5E50 branch. This does not collect a production signature.

#[cfg(any(all(windows, target_arch = "x86_64"), test))]
mod fields {
    const DESCRIPTOR_PREFIX: usize = 36;
    const MEDIA_PREFIX: usize = 16;
    const SOURCE_CAPACITY: usize = 1000;
    const RESULT_CAPACITY: usize = 260;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Invalid {
        Header,
        Size,
        Offset,
        Terminator,
        DecodeTail,
        MediaStatus,
        SourceTooLong,
        DestinationTooLong,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Shape {
        Empty,
        WordSwapped,
        Raw,
        UppercaseHex,
    }

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().expect("checked header"))
    }

    fn c_string(bytes: &[u8]) -> Result<&[u8], Invalid> {
        let end = bytes
            .iter()
            .position(|b| *b == 0)
            .ok_or(Invalid::Terminator)?;
        Ok(&bytes[..end])
    }

    fn text_bytes(bytes: &[u8]) -> bool {
        bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'-'))
    }

    fn nibble(byte: u8) -> u8 {
        // CF669C maps invalid entries to the no-add arm; out-of-table
        // characters also leave the just-shifted value unchanged.
        match byte {
            b'0'..=b'9' => byte - b'0',
            b'A'..=b'F' => byte - b'A' + 10,
            b'a'..=b'f' => byte - b'a' + 10,
            _ => 0,
        }
    }

    fn decode_words(bytes: &[u8]) -> Vec<u8> {
        let mut result = Vec::with_capacity(bytes.len() / 2);
        for word in bytes.chunks_exact(4) {
            for start in [2, 0] {
                let byte = (nibble(word[start]) << 4) | nibble(word[start + 1]);
                if byte != 0 {
                    result.push(byte);
                }
            }
        }
        result
    }

    fn finish(bytes: &[u8], source: Shape) -> Result<(Shape, Vec<u8>), Invalid> {
        if bytes.is_empty() {
            return Ok((Shape::Empty, Vec::new()));
        }
        if bytes.len() >= SOURCE_CAPACITY {
            return Err(Invalid::SourceTooLong);
        }
        if text_bytes(bytes) {
            if bytes.len() >= RESULT_CAPACITY {
                return Err(Invalid::DestinationTooLong);
            }
            return Ok((source, bytes.into()));
        }

        // CF4540 formats each original byte with %02X. CF5E50 trims this
        // expanded result to the 260-byte destination less its terminator.
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let escaped = bytes
            .iter()
            .flat_map(|b| [HEX[usize::from(b >> 4)], HEX[usize::from(b & 15)]])
            .take(RESULT_CAPACITY - 1)
            .collect();
        Ok((Shape::UppercaseHex, escaped))
    }

    fn descriptor_choice(bytes: &[u8]) -> Result<(Shape, Vec<u8>), Invalid> {
        if bytes.len() < DESCRIPTOR_PREFIX {
            return Err(Invalid::Header);
        }
        let version = u32_at(bytes, 0) as usize;
        let size = u32_at(bytes, 4) as usize;
        if version < DESCRIPTOR_PREFIX || size < version || size > bytes.len() {
            return Err(Invalid::Size);
        }
        let offset = u32_at(bytes, 24) as usize;
        if offset == 0 {
            return Ok((Shape::Empty, Vec::new()));
        }
        if !(DESCRIPTOR_PREFIX..size).contains(&offset) {
            return Err(Invalid::Offset);
        }
        let tail = &bytes[offset..size];
        let raw = c_string(tail)?;
        if raw.len() >= SOURCE_CAPACITY {
            return Err(Invalid::SourceTooLong);
        }
        if raw.len() < 2 {
            return finish(raw, Shape::Raw);
        }

        // The recovered loop visits full four-byte words even when the C
        // string ends within a word. Require that those bytes really belong
        // to the returned descriptor. Do not invent padding or read past it.
        let word_bytes = tail
            .get(..raw.len().next_multiple_of(4))
            .ok_or(Invalid::DecodeTail)?;
        let decoded = decode_words(word_bytes);
        if decoded.get(..raw.len() / 2).is_some_and(text_bytes) {
            finish(&decoded, Shape::WordSwapped)
        } else {
            finish(raw, Shape::Raw)
        }
    }

    fn media_choice(bytes: &[u8]) -> Result<(Shape, Vec<u8>), Invalid> {
        if bytes.len() < MEDIA_PREFIX {
            return Err(Invalid::Header);
        }
        let length = u32_at(bytes, 0) as usize;
        if u32_at(bytes, 4) != 0 {
            return Err(Invalid::MediaStatus);
        }
        let body = bytes[MEDIA_PREFIX..].get(..length).ok_or(Invalid::Size)?;
        // The documented length is authoritative in this probe. A terminator
        // is optional when length already bounds the serial; reject a suffix
        // that would otherwise be silently discarded.
        let serial = match body.iter().position(|b| *b == 0) {
            Some(i) if body[i..].iter().all(|b| *b == 0) => &body[..i],
            Some(_) => return Err(Invalid::Terminator),
            None => body,
        };
        finish(serial, Shape::Raw)
    }

    pub(super) fn descriptor(bytes: &[u8]) -> Result<Shape, Invalid> {
        descriptor_choice(bytes).map(|(shape, _)| shape)
    }

    pub(super) fn media(bytes: &[u8]) -> Result<Shape, Invalid> {
        media_choice(bytes).map(|(shape, _)| shape)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn descriptor_fixture(raw: &[u8], extra: &[u8]) -> Vec<u8> {
            let mut reply = vec![0; 40];
            reply.extend_from_slice(raw);
            reply.push(0);
            reply.extend_from_slice(extra);
            let size = reply.len() as u32;
            reply[..4].copy_from_slice(&40u32.to_le_bytes());
            reply[4..8].copy_from_slice(&size.to_le_bytes());
            reply[24..28].copy_from_slice(&40u32.to_le_bytes());
            reply
        }

        #[test]
        fn descriptor_distinguishes_word_order_raw_and_recovered_hex_escape() {
            assert_eq!(
                descriptor_choice(&descriptor_fixture(b"32313433", &[])),
                Ok((Shape::WordSwapped, b"1234".to_vec()))
            );
            assert_eq!(
                descriptor_choice(&descriptor_fixture(b"Az9-Lab ", &[])),
                Ok((Shape::Raw, b"Az9-Lab ".to_vec()))
            );
            assert_eq!(
                descriptor_choice(&descriptor_fixture(b"A_B#", &[])),
                Ok((Shape::UppercaseHex, b"415F4223".to_vec()))
            );
            // Non-hex nibbles become zero. The missing decoded byte
            // invalidates the expected prefix rather than being padded.
            assert_eq!(
                descriptor_choice(&descriptor_fixture(b"003131zz", &[])),
                Ok((Shape::Raw, b"003131zz".to_vec()))
            );
            // A partial word can use its following returned byte only
            // when the bounded descriptor actually contains that byte.
            assert_eq!(
                descriptor_choice(&descriptor_fixture(b"31", &[0])),
                Ok((Shape::WordSwapped, b"1".to_vec()))
            );
            assert_eq!(
                descriptor(&descriptor_fixture(b"31", &[])),
                Err(Invalid::DecodeTail)
            );
        }

        #[test]
        fn descriptor_rejects_header_offset_size_and_nul_faults() {
            let mut reply = descriptor_fixture(b"test", &[]);
            assert_eq!(descriptor(&reply[..35]), Err(Invalid::Header));
            assert_eq!(descriptor(&reply[..44]), Err(Invalid::Size));
            *reply.last_mut().unwrap() = b'z';
            assert_eq!(descriptor(&reply), Err(Invalid::Terminator));
            for offset in [1u32, 35, 45, u32::MAX] {
                reply[24..28].copy_from_slice(&offset.to_le_bytes());
                assert_eq!(descriptor(&reply), Err(Invalid::Offset));
            }
            reply[24..28].fill(0);
            assert_eq!(descriptor(&reply), Ok(Shape::Empty));
        }

        #[test]
        fn bounded_copy_and_cleanup_keep_the_native_destination_limit() {
            assert_eq!(
                descriptor(&descriptor_fixture(&[b'Z'; 260], &[])),
                Err(Invalid::DestinationTooLong)
            );
            assert_eq!(
                descriptor(&descriptor_fixture(&[b'Z'; 1000], &[])),
                Err(Invalid::SourceTooLong)
            );
            let (shape, escaped) =
                descriptor_choice(&descriptor_fixture(&[b'?'; 160], &[])).unwrap();
            assert_eq!(shape, Shape::UppercaseHex);
            assert_eq!(escaped.len(), 259);
            assert!(
                escaped
                    .iter()
                    .enumerate()
                    .all(|(i, b)| *b == if i % 2 == 0 { b'3' } else { b'F' })
            );
        }

        #[test]
        fn media_honors_status_and_length_instead_of_scanning_the_zero_filled_buffer() {
            let mut reply = vec![0; 16];
            reply[..4].copy_from_slice(&6u32.to_le_bytes());
            reply.extend_from_slice(b"Lab-42extra");
            assert_eq!(media_choice(&reply), Ok((Shape::Raw, b"Lab-42".to_vec())));
            assert_eq!(media(&reply[..21]), Err(Invalid::Size));
            reply[4] = 1;
            assert_eq!(media(&reply), Err(Invalid::MediaStatus));
            reply[4] = 0;
            reply[17] = 0;
            assert_eq!(media(&reply), Err(Invalid::Terminator));
            reply[..4].fill(0);
            assert_eq!(media(&reply), Ok(Shape::Empty));
        }
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows {
    use super::fields::{self, Invalid, Shape};
    use std::{ffi::c_void, ptr::null_mut, time::Duration};

    const FILE_SHARE_READ_WRITE: u32 = 3;
    const OPEN_EXISTING: u32 = 3;
    const QUERY_PROPERTY: u32 = 0x002d1400;
    const MEDIA_SERIAL: u32 = 0x002d0c10;
    const REPLY_SIZE: usize = 0x2710;
    const SLOTS: u32 = 16;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateFileW(
            file: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            disposition: u32,
            flags: u32,
            template: *mut c_void,
        ) -> *mut c_void;
        fn DeviceIoControl(
            handle: *mut c_void,
            code: u32,
            input: *const c_void,
            input_len: u32,
            output: *mut c_void,
            output_len: u32,
            returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn GetLastError() -> u32;
    }

    #[derive(Clone, Copy)]
    enum MetadataQuery {
        Descriptor,
        Media,
    }

    impl MetadataQuery {
        fn code(self) -> u32 {
            match self {
                Self::Descriptor => QUERY_PROPERTY,
                Self::Media => MEDIA_SERIAL,
            }
        }

        fn input(self) -> &'static [u32] {
            match self {
                Self::Descriptor => &[0; 3],
                Self::Media => &[],
            }
        }

        fn inspect(self, bytes: &[u8]) -> Result<Shape, Invalid> {
            match self {
                Self::Descriptor => fields::descriptor(bytes),
                Self::Media => fields::media(bytes),
            }
        }
    }

    struct Disk(*mut c_void);
    impl Disk {
        fn open(index: u32) -> Result<Self, u32> {
            debug_assert!(index < SLOTS);
            let name: Vec<_> = format!(r"\\.\PhysicalDrive{index}")
                .encode_utf16()
                .chain([0])
                .collect();
            // SAFETY: the fixed generated UTF-16 name is terminated; there
            // is no raw read/write access, creation, inheritance or template.
            unsafe {
                let handle = CreateFileW(
                    name.as_ptr(),
                    0,
                    FILE_SHARE_READ_WRITE,
                    null_mut(),
                    OPEN_EXISTING,
                    0,
                    null_mut(),
                );
                if handle.is_null() || handle as isize == -1 {
                    Err(GetLastError())
                } else {
                    Ok(Self(handle))
                }
            }
        }

        fn query(&self, request: MetadataQuery) -> Outcome {
            let input = request.input();
            #[repr(align(8))]
            struct Buffer([u8; REPLY_SIZE]);
            let mut output = Buffer([0; REPLY_SIZE]);
            let mut returned = 0;
            // SAFETY: the input is either the initialized 12-byte property
            // query or null; output and returned are writable for the
            // synchronous call. No overlapped state outlives the buffers.
            let ok = unsafe {
                DeviceIoControl(
                    self.0,
                    request.code(),
                    if input.is_empty() {
                        std::ptr::null()
                    } else {
                        input.as_ptr().cast()
                    },
                    std::mem::size_of_val(input) as u32,
                    output.0.as_mut_ptr().cast(),
                    REPLY_SIZE as u32,
                    &mut returned,
                    null_mut(),
                )
            };
            if ok == 0 {
                // SAFETY: this thread just returned from the failed Win32 call.
                return Outcome::Os(unsafe { GetLastError() });
            }
            let Some(data) = output.0.get(..returned as usize) else {
                return Outcome::Malformed(Invalid::Size);
            };
            match request.inspect(data) {
                Ok(shape) => Outcome::Parsed(shape),
                Err(error) => Outcome::Malformed(error),
            }
        }
    }

    impl Drop for Disk {
        fn drop(&mut self) {
            // SAFETY: the exclusively owned handle was returned successfully
            // by CreateFileW and is closed exactly once here.
            unsafe { CloseHandle(self.0) };
        }
    }

    enum Outcome {
        Os(u32),
        Malformed(Invalid),
        Parsed(Shape),
    }

    #[derive(Default)]
    struct Counter {
        empty: u32,
        words: u32,
        raw: u32,
        escaped: u32,
        malformed: u32,
        first_parser_rejection: Option<Invalid>,
        access_denied: u32,
        unsupported: u32,
        other_os_error: u32,
    }

    impl Counter {
        fn record(&mut self, result: Outcome) {
            match result {
                Outcome::Parsed(Shape::Empty) => self.empty += 1,
                Outcome::Parsed(Shape::WordSwapped) => self.words += 1,
                Outcome::Parsed(Shape::Raw) => self.raw += 1,
                Outcome::Parsed(Shape::UppercaseHex) => self.escaped += 1,
                Outcome::Malformed(error) => {
                    self.malformed += 1;
                    self.first_parser_rejection.get_or_insert(error);
                }
                Outcome::Os(5) => self.access_denied += 1,
                Outcome::Os(1 | 50 | 87) => self.unsupported += 1,
                Outcome::Os(_) => self.other_os_error += 1,
            }
        }

        fn present(&self) -> u32 {
            self.words + self.raw + self.escaped
        }

        fn print(&self, label: &str) {
            println!(
                "{label} empty={} words={} raw={} escaped={} malformed={} access_denied={} unsupported={} other_os_error={} first_parser_rejection={:?}",
                self.empty,
                self.words,
                self.raw,
                self.escaped,
                self.malformed,
                self.access_denied,
                self.unsupported,
                self.other_os_error,
                self.first_parser_rejection,
            );
        }
    }

    struct Report {
        opened: u32,
        absent: u32,
        denied: u32,
        failed: u32,
        descriptor: Counter,
        media: Counter,
    }

    fn observe() -> Report {
        let mut report = Report {
            opened: 0,
            absent: 0,
            denied: 0,
            failed: 0,
            descriptor: Counter::default(),
            media: Counter::default(),
        };
        for index in 0..SLOTS {
            let disk = match Disk::open(index) {
                Ok(disk) => disk,
                Err(2 | 3 | 21) => {
                    report.absent += 1;
                    continue;
                }
                Err(5) => {
                    report.denied += 1;
                    continue;
                }
                Err(_) => {
                    report.failed += 1;
                    continue;
                }
            };
            report.opened += 1;
            report
                .descriptor
                .record(disk.query(MetadataQuery::Descriptor));
            report.media.record(disk.query(MetadataQuery::Media));
        }
        report
    }

    pub(super) fn run() -> i32 {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let started = std::thread::Builder::new()
            .name("offline-storage-metadata".into())
            .spawn(move || {
                let _ = send.send(observe());
            });
        if started.is_err() {
            eprintln!("storage_probe=unavailable");
            return 3;
        }
        let Ok(report) = receive.recv_timeout(Duration::from_secs(5)) else {
            // This is a standalone process. Exiting drops the diagnostic
            // rather than returning a partial result or trying another handle.
            eprintln!("storage_probe=timed_out_or_stopped");
            return 3;
        };
        println!(
            "drive_slots={SLOTS} opened={} absent={} access_denied={} other_open_error={}",
            report.opened, report.absent, report.denied, report.failed
        );
        report.descriptor.print("descriptor");
        report.media.print("media");
        if report.descriptor.present() + report.media.present() == 0 {
            println!("storage_probe=no_readable_serial");
            2
        } else {
            println!("storage_probe=observed_metadata_only");
            0
        }
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn main() {
    if !std::env::args_os()
        .skip(1)
        .eq([std::ffi::OsString::from("--offline")])
    {
        eprintln!("usage: windows_storage_descriptor_probe.exe --offline");
        std::process::exit(1);
    }
    std::process::exit(windows::run());
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn main() {
    eprintln!("storage_probe=requires_windows_x86_64");
    std::process::exit(2);
}
