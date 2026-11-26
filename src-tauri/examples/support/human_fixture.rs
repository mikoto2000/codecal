use sha2::{Digest, Sha256};

pub(crate) fn native_id(value: usize, uuid: bool) -> String {
    if !uuid {
        return format!("{value:012}");
    }
    let digest = Sha256::digest((value as u64).to_le_bytes());
    let mut bytes: [u8; 16] = digest[..16].try_into().expect("sixteen digest bytes");
    bytes[6] = (bytes[6] & 15) | 0x40;
    bytes[8] = (bytes[8] & 63) | 0x80;
    let mut out = String::with_capacity(36);
    for (i, byte) in bytes.into_iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        use std::fmt::Write;
        write!(&mut out, "{byte:02x}").expect("string write");
    }
    out
}
pub(crate) fn normal_payload(duration: i64) -> String {
    let make = |padding: &str| {
        serde_json::json!({"durationEstimate":duration,"foreground":{"title":format!("SYNTHETIC_PRIVATE_CONTENT{padding}")},"detection":{"diagnostics":{"entertainmentDisposition":"NON_ENTERTAINMENT"}}}).to_string()
    };
    let initial = make("");
    make(&"x".repeat(200 - initial.len()))
}
