import re

with open("crates/photohelper-sidecar/src/reader.rs", "r") as f:
    text = f.read()

text = text.replace(
    ") -> Result<(), Error> {",
    ") {"
)
text = text.replace(
    "    Ok(())\n}\n\npub(crate) fn parse_xmp_str",
    "}\n\npub(crate) fn parse_xmp_str"
)
text = text.replace(
    "apply_parsed_field(fields, metadata_date, prefix, local_key, val.trim(), path)?;",
    "apply_parsed_field(fields, metadata_date, prefix, local_key, val.trim(), path);"
)
text = text.replace(
"""                            apply_parsed_field(
                                &mut fields,
                                &mut metadata_date,
                                prefix,
                                local,
                                &text,
                                path,
                            )?;""",
"""                            apply_parsed_field(
                                &mut fields,
                                &mut metadata_date,
                                prefix,
                                local,
                                &text,
                                path,
                            );"""
)
with open("crates/photohelper-sidecar/src/reader.rs", "w") as f:
    f.write(text)

with open("crates/photohelper-sidecar/src/writer.rs", "r") as f:
    writer = f.read()
writer = writer.replace("pub fn is_valid_xml_string", "#[allow(dead_code)]\npub fn is_valid_xml_string")
with open("crates/photohelper-sidecar/src/writer.rs", "w") as f:
    f.write(writer)

with open("crates/photohelper-sidecar/src/xml.rs", "r") as f:
    xml = f.read()
xml = xml.replace("pub(crate) fn sanitize_xml_string", "#[allow(dead_code)]\npub(crate) fn sanitize_xml_string")
with open("crates/photohelper-sidecar/src/xml.rs", "w") as f:
    f.write(xml)
