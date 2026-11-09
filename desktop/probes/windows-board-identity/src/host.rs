use crate::{
    host_shape::{ARRAY_I4, ARRAY_UI2, bounded_rank_one, integer_array},
    wmi::{Failure, Property, ReadState, acp_with_terminal, first_property},
};
use std::{fmt, mem::offset_of};
use windows::{
    Win32::{
        Globalization::GetACP,
        System::{
            Com::SAFEARRAY,
            Ole::{
                SafeArrayGetDim, SafeArrayGetElemsize, SafeArrayGetLBound, SafeArrayGetUBound,
                SafeArrayGetVartype,
            },
            Variant::{VT_ARRAY, VT_I4, VT_NULL, VT_UI2},
            Wmi::IWbemServices,
        },
    },
    core::{PCWSTR, w},
};

const PRODUCT_QUERY: &str = "SELECT * FROM Win32_ComputerSystemProduct";
const ENCLOSURE_QUERY: &str = "SELECT * FROM Win32_SystemEnclosure";

// D022B7's qword at descriptor +8 is NOT the first element or its pointer.
const _: () = assert!(offset_of!(SAFEARRAY, cLocks) == 8);
const _: () = assert!(offset_of!(SAFEARRAY, pvData) == 16);
const _: () = assert!(ARRAY_I4 == (VT_ARRAY.0 | VT_I4.0));
const _: () = assert!(ARRAY_UI2 == (VT_ARRAY.0 | VT_UI2.0));

enum Encoding {
    NotRead,
    Failed,
    Empty,
    Present,
}

struct Field {
    read: ReadState,
    detail: Option<i32>,
    encoding: Encoding,
}
impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let encoding = match self.encoding {
            Encoding::NotRead => "not_read",
            Encoding::Failed => "failed",
            Encoding::Empty => "empty",
            Encoding::Present => "present",
        };
        write!(f, "{:?}:{:?}:{encoding}", self.read, self.detail)
    }
}

fn product_field(services: &IWbemServices, property: PCWSTR) -> Field {
    // Separate query/first row for each property, as D01400 calls CFE940 twice.
    let encoded = first_property(services, PRODUCT_QUERY, property)
        .and_then(|value| value.bounded_bstr().map(acp_with_terminal));
    match encoded {
        Ok(bytes) => Field {
            read: ReadState::Present,
            detail: None,
            encoding: match bytes {
                Some(bytes) if bytes[0] == 0 => Encoding::Empty,
                Some(_) => Encoding::Present,
                None => Encoding::Failed,
            },
        },
        Err(Failure(read, detail)) => Field {
            read,
            detail,
            encoding: Encoding::NotRead,
        },
    }
}

#[derive(Clone, Copy, Debug)]
enum Shape {
    Null,
    UnsupportedTag,
    NullArray,
    UnsupportedRank,
    UnsupportedElement,
    UnknownBounds,
    OutOfBounds,
    CheckedMetadata,
}

fn enclosure_shape(property: &Property) -> (Shape, u16) {
    let tagged = property.tagged();
    let tag = tagged.vt.0;
    if tagged.vt == VT_NULL {
        return (Shape::Null, tag);
    }
    let Some((element, width)) = integer_array(tag) else {
        // Includes BYREF, variant arrays, and unproven scalar shapes.
        return (Shape::UnsupportedTag, tag);
    };
    // SAFETY: exact non-BYREF array tag, borrowed while Property owns it.
    let array = unsafe { tagged.Anonymous.parray };
    if array.is_null() {
        return (Shape::NullArray, tag);
    }
    // SAFETY: OleAut32 examines metadata of this live SAFEARRAY. We neither
    // read raw layout/padding nor access/lock/copy any array elements.
    if unsafe { SafeArrayGetDim(array) } != 1 {
        return (Shape::UnsupportedRank, tag);
    }
    if unsafe { SafeArrayGetElemsize(array) } != width
        || unsafe { SafeArrayGetVartype(array) }
            .ok()
            .map(|kind| kind.0)
            != Some(element)
    {
        return (Shape::UnsupportedElement, tag);
    }
    let (Ok(lower), Ok(upper)) = (unsafe { SafeArrayGetLBound(array, 1) }, unsafe {
        SafeArrayGetUBound(array, 1)
    }) else {
        return (Shape::UnknownBounds, tag);
    };
    if !bounded_rank_one(lower, upper) {
        return (Shape::OutOfBounds, tag);
    }
    (Shape::CheckedMetadata, tag)
}

pub(super) struct Report {
    acp: u32,
    vendor: Field,
    name: Field,
    enclosure: (ReadState, Option<i32>, Option<(Shape, u16)>),
}
impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "host_probe=complete acp={} vendor={} name={} enclosure={:?} host_description_complete=false private_values_emitted=0",
            self.acp, self.vendor, self.name, self.enclosure
        )
    }
}

pub(super) fn observe(services: &IWbemServices) -> Report {
    let vendor = product_field(services, w!("Vendor"));
    let name = product_field(services, w!("Name"));
    let enclosure = match first_property(services, ENCLOSURE_QUERY, w!("ChassisTypes")) {
        Ok(value) => (ReadState::Present, None, Some(enclosure_shape(&value))),
        Err(Failure(read, detail)) => (read, detail, None),
    };
    // SAFETY: read-only process code-page query.
    let acp = unsafe { GetACP() };
    Report {
        acp,
        vendor,
        name,
        enclosure,
    }
}
