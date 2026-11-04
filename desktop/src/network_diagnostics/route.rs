use cumulus_core::network_diagnostics::DefaultRoute;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{fs::File, io::Read};

    const UP: u32 = 1;
    const REJECT: u32 = 0x200;
    const TABLE_LIMIT: u64 = 1024 * 1024;

    fn active(flags: &str) -> Option<bool> {
        let flags = u32::from_str_radix(flags, 16).ok()?;
        Some(flags & UP != 0 && flags & REJECT == 0)
    }

    fn has_v4_default(table: &str) -> Option<bool> {
        let mut lines = table.lines();
        if !lines.next()?.starts_with("Iface") {
            return None;
        }
        for line in lines {
            let mut columns = line.split_ascii_whitespace();
            let [interface, target, _, flags, _, _, _, mask] =
                std::array::from_fn(|_| columns.next());
            if target? == "00000000" && mask? == "00000000" && interface? != "lo" && active(flags?)?
            {
                return Some(true);
            }
        }
        Some(false)
    }

    fn has_v6_default(table: &str) -> Option<bool> {
        for line in table.lines() {
            let mut columns = line.split_ascii_whitespace();
            let [destination, prefix, _, _, _, _, _, _, flags, interface] =
                std::array::from_fn(|_| columns.next());
            // The kernel may provide a reject route on lo when there is no IPv6 default.
            if destination? == "00000000000000000000000000000000"
                && prefix? == "00"
                && interface? != "lo"
                && active(flags?)?
            {
                return Some(true);
            }
        }
        Some(false)
    }

    fn table(path: &str, parse: fn(&str) -> Option<bool>) -> Option<bool> {
        let mut input = String::new();
        let bytes = File::open(path)
            .ok()?
            .take(TABLE_LIMIT + 1)
            .read_to_string(&mut input)
            .ok()?;
        (bytes as u64 <= TABLE_LIMIT)
            .then(|| parse(&input))
            .flatten()
    }

    pub(super) fn default_route() -> DefaultRoute {
        match (
            table("/proc/net/route", has_v4_default),
            table("/proc/net/ipv6_route", has_v6_default),
        ) {
            (Some(true), _) | (_, Some(true)) => DefaultRoute::Present,
            (Some(false), Some(false)) => DefaultRoute::Missing,
            _ => DefaultRoute::Unavailable,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn only_active_default_routes_count_without_returning_route_data() {
            let header = "Iface Destination Gateway Flags RefCnt Use Metric Mask\n";
            assert_eq!(has_v4_default(header), Some(false));
            assert_eq!(
                has_v4_default(&format!(
                    "{header}x 00000000 0102ABCD 0003 0 0 100 00000000"
                )),
                Some(true)
            );
            assert_eq!(
                has_v4_default(&format!(
                    "{header}x 00000000 0102ABCD 0203 0 0 100 00000000"
                )),
                Some(false)
            );
            assert_eq!(
                has_v4_default(&format!(
                    "{header}x 00000000 0102ABCD 0003 0 0 100 0000FFFF"
                )),
                Some(false)
            );
            assert_eq!(has_v4_default("unavailable"), None);
            let destination = "00000000000000000000000000000000";
            let prefix = format!("{destination} 00 {destination} 00 {destination} 0 0 0");
            assert_eq!(
                has_v6_default(&format!("{prefix} 00200200 lo")),
                Some(false)
            );
            assert_eq!(has_v6_default(&format!("{prefix} 00000001 x")), Some(true));
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) fn default_route() -> DefaultRoute {
    linux::default_route()
}

#[cfg(windows)]
pub(super) fn default_route() -> DefaultRoute {
    use std::ptr::{NonNull, addr_of, null_mut};
    use windows_sys::Win32::{
        NetworkManagement::IpHelper::{FreeMibTable, GetIpForwardTable2, MIB_IPFORWARD_TABLE2},
        Networking::WinSock::AF_UNSPEC,
    };

    struct RouteTable(NonNull<MIB_IPFORWARD_TABLE2>);
    impl Drop for RouteTable {
        fn drop(&mut self) {
            // SAFETY: IP Helper allocated this table and requires FreeMibTable once.
            unsafe { FreeMibTable(self.0.as_ptr().cast()) }
        }
    }

    let mut raw = null_mut();
    // SAFETY: the out parameter lives for the call. AF_UNSPEC requests both families.
    if unsafe { GetIpForwardTable2(AF_UNSPEC, &mut raw) } != 0 {
        return DefaultRoute::Unavailable;
    }
    let Some(raw) = NonNull::new(raw) else {
        return DefaultRoute::Unavailable;
    };
    let table = RouteTable(raw);
    // SAFETY: success returns a live table header owned by RouteTable.
    let count = unsafe { addr_of!((*table.0.as_ptr()).NumEntries).read() } as usize;
    if count == 0 {
        return DefaultRoute::Missing;
    }
    if count > 16_384 {
        return DefaultRoute::Unavailable;
    }
    // SAFETY: the API supplies NumEntries initialized rows in the trailing table
    // array. The pointer is aligned and the capped count is within isize::MAX.
    let rows = unsafe {
        let first = addr_of!((*table.0.as_ptr()).Table).cast();
        std::slice::from_raw_parts(first, count)
    };
    if rows.iter().any(
        |row: &windows_sys::Win32::NetworkManagement::IpHelper::MIB_IPFORWARD_ROW2| {
            row.DestinationPrefix.PrefixLength == 0 && row.ValidLifetime != 0 && !row.Loopback
        },
    ) {
        DefaultRoute::Present
    } else {
        DefaultRoute::Missing
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
pub(super) fn default_route() -> DefaultRoute {
    DefaultRoute::Unavailable
}
