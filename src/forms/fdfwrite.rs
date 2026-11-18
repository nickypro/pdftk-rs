//! Port of iText FdfWriter (as patched by pdftk) — the generate_fdf output.
use super::util::*;
use crate::{pdf, report};
use std::io::Write;

enum Node {
    Map(JavaHashMap<Node>),
    Str(String),
    Name(String),
}

pub struct FdfWriter {
    fields: JavaHashMap<Node>,
}

impl FdfWriter {
    pub fn new() -> Self {
        FdfWriter { fields: JavaHashMap::new() }
    }

    /// FdfWriter.setField(): `field` is split like `new StringTokenizer(field, ".")`.
    fn set_field(&mut self, field: &str, value: Node) -> bool {
        let tokens: Vec<&str> = field.split('.').filter(|t| !t.is_empty()).collect();
        if tokens.is_empty() {
            return false;
        }
        let mut map = &mut self.fields;
        for (i, s) in tokens.iter().enumerate() {
            let last = i + 1 == tokens.len();
            if !last {
                if !map.contains_key(s) {
                    map.put(s, Node::Map(JavaHashMap::new()));
                }
                match map.get_mut(s) {
                    Some(Node::Map(m)) => map = m,
                    _ => return false,
                }
            } else {
                if let Some(Node::Map(_)) = map.get(s) {
                    return false;
                }
                map.put(s, value);
                return true;
            }
        }
        false
    }

    pub fn set_field_as_name(&mut self, field: &str, value: &str) -> bool {
        self.set_field(field, Node::Name(value.to_string()))
    }

    pub fn set_field_as_string(&mut self, field: &str, value: &str) -> bool {
        self.set_field(field, Node::Str(value.to_string()))
    }

    /// FdfWriter.writeTo() / Wrt.writeTo(), with pdftk's dictionary formatting.
    pub fn write_to(&self, os: &mut dyn Write) -> std::io::Result<()> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"%FDF-1.2\n%\xe2\xe3\xcf\xd3\n");
        out.extend_from_slice(b"1 0 obj\n");
        out.extend_from_slice(b"\n<<\n/FDF \n<<\n/Fields ");
        calculate(&self.fields, &mut out);
        out.extend_from_slice(b"\n>>\n>>");
        out.extend_from_slice(b"\nendobj\n");
        out.extend_from_slice(b"\ntrailer\n");
        out.extend_from_slice(b"\n<<\n/Root 1 0 R\n>>");
        out.extend_from_slice(b"\n%%EOF\n");
        os.write_all(&out)
    }
}

fn pdf_string(s: &str, out: &mut Vec<u8>) {
    out.extend_from_slice(&escape_string(&pdf::unicode_to_text_string(s)));
}

fn calculate(map: &JavaHashMap<Node>, out: &mut Vec<u8>) {
    out.push(b'[');
    let mut first = true;
    for (key, v) in map.iter() {
        if !first {
            out.push(b' ');
        }
        first = false;
        out.extend_from_slice(b"\n<<\n/T ");
        pdf_string(key, out);
        match v {
            Node::Map(m) => {
                out.extend_from_slice(b"\n/Kids ");
                calculate(m, out);
            }
            Node::Str(s) => {
                out.extend_from_slice(b"\n/V ");
                pdf_string(s, out);
            }
            Node::Name(n) => {
                out.extend_from_slice(b"\n/V ");
                out.extend_from_slice(&report::itext_name_bytes(&string_to_name(n)));
            }
        }
        out.extend_from_slice(b"\n>>");
    }
    out.push(b']');
}
