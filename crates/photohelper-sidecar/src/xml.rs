pub(crate) fn is_valid_xml_char(c: char) -> bool {
    let val = c as u32;
    let is_valid_xml_char = (0x20..=0xD7FF).contains(&val)
        || val == 0x09
        || val == 0x0A
        || val == 0x0D
        || (0xE000..=0xFFFD).contains(&val)
        || (0x10000..=0x10_FFFF).contains(&val);
    let is_noncharacter = (0xFDD0..=0xFDEF).contains(&val) || (val & 0xFFFE) == 0xFFFE;
    is_valid_xml_char && !is_noncharacter
}

/// Returns true if the string contains only valid XML 1.0 characters.
pub fn is_valid_xml_string(s: &str) -> bool {
    s.chars().all(is_valid_xml_char)
}

#[allow(dead_code)]
pub(crate) fn sanitize_xml_string(s: &str) -> String {
    s.chars().filter(|&c| is_valid_xml_char(c)).collect()
}
