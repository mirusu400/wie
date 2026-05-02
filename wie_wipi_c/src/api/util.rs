use alloc::{format, string::String, vec::Vec};

use wie_util::{Result, read_null_terminated_string_bytes};

use wipi_types::wipic::WIPICWord;

use crate::context::WIPICContext;

const M_E_INVALID: i32 = -9;

pub async fn htonl(_context: &mut dyn WIPICContext, val: WIPICWord) -> Result<WIPICWord> {
    Ok(val.to_be())
}

pub async fn htons(_context: &mut dyn WIPICContext, val: WIPICWord) -> Result<WIPICWord> {
    Ok((val as u16).to_be() as _)
}

pub async fn ntohl(_context: &mut dyn WIPICContext, val: WIPICWord) -> Result<WIPICWord> {
    Ok(WIPICWord::from_be(val))
}

pub async fn ntohs(_context: &mut dyn WIPICContext, val: WIPICWord) -> Result<WIPICWord> {
    Ok(u16::from_be(val as u16) as _)
}

pub async fn inet_addr_int(context: &mut dyn WIPICContext, addr_ptr: WIPICWord) -> Result<i32> {
    let raw = read_null_terminated_string_bytes(context, addr_ptr)?;
    let s = String::from_utf8(raw).unwrap_or_default();
    let octets: Vec<u8> = s.split('.').flat_map(|p| p.parse::<u8>().ok()).collect();
    if octets.len() != 4 {
        return Ok(M_E_INVALID);
    }
    Ok(i32::from_be_bytes([octets[0], octets[1], octets[2], octets[3]]))
}

pub async fn inet_addr_str(context: &mut dyn WIPICContext, ip: i32, addr_ptr: WIPICWord) -> Result<()> {
    let bytes = (ip as u32).to_be_bytes();
    let formatted = format!("{}.{}.{}.{}\0", bytes[0], bytes[1], bytes[2], bytes[3]);
    context.write_bytes(addr_ptr, formatted.as_bytes())?;
    Ok(())
}
