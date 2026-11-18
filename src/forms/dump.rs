//! Port of report.ReportAcroFormFields() and its helpers (dump_data_fields[_utf8]).
use super::util::*;
use crate::report;
use lopdf::{Document, Object};
use std::io::Write;

/// java.util.LinkedHashSet<String>
#[derive(Clone, Default)]
struct LinkedSet<T: PartialEq + Clone>(Vec<T>);
impl<T: PartialEq + Clone> LinkedSet<T> {
    fn add(&mut self, v: T) {
        if !self.0.contains(&v) {
            self.0.push(v);
        }
    }
}

/// report.FormField
#[derive(Clone)]
struct FormField {
    m_ft: String,
    m_tt: String,
    m_tu: String,
    m_ff: i32,
    m_vv: LinkedSet<String>,
    m_dv: String,
    m_qq: i32,
    m_ds: String,
    m_rv: Vec<u8>,
    m_maxlen: i32,
    m_have_option_list: bool,
    m_states: LinkedSet<String>,
    m_states_value_display: LinkedSet<Vec<String>>,
}

impl FormField {
    fn new() -> Self {
        FormField {
            m_ft: String::new(),
            m_tt: String::new(),
            m_tu: String::new(),
            m_ff: 0,
            m_vv: LinkedSet(Vec::new()),
            m_dv: String::new(),
            m_qq: 0,
            m_ds: String::new(),
            m_rv: Vec::new(),
            m_maxlen: 0,
            m_have_option_list: false,
            m_states: LinkedSet(Vec::new()),
            m_states_value_display: LinkedSet(Vec::new()),
        }
    }

    fn add_options(&mut self, doc: &Document, opts: &[Object], utf8_b: bool) {
        for o in opts {
            let opt = match res(doc, Some(o)) {
                Some(x) => x,
                None => continue,
            };
            match opt {
                Object::String(..) => self.m_states.add(report::output_pdf_string(Some(opt), utf8_b)),
                Object::Array(sub) => {
                    // note: the Java does not resolve references inside the pair
                    let pair: Vec<String> = sub
                        .iter()
                        .filter(|s| matches!(s, Object::String(..)))
                        .map(|s| report::output_pdf_string(Some(s), utf8_b))
                        .collect();
                    if pair.len() != 2 {
                        continue;
                    }
                    self.m_states_value_display.add(pair);
                }
                _ => {}
            }
        }
    }

    fn add_ap_states(&mut self, doc: &Document, ap: &lopdf::Dictionary, state_name: &[u8], utf8_b: bool) {
        if !ap.has(state_name) {
            return;
        }
        if let Some(Object::Dictionary(n)) = res(doc, ap.get(state_name).ok()) {
            for key in report::itext_dict_keys(n) {
                if !self.m_have_option_list || key.as_slice() == b"Off" {
                    self.m_states.add(report::output_pdf_name(Some(&Object::Name(key.clone())), utf8_b));
                }
            }
        }
    }

    fn interpret_value(&self, v: &str) -> String {
        if self.m_ft == "Button" && self.m_have_option_list {
            // V is possibly not the value but an index to the Opt array
            if let Some(i) = java_parse_int(v) {
                if i >= 0 && (i as usize) < self.m_states.0.len() {
                    return self.m_states.0[i as usize].clone();
                }
            }
        }
        v.to_string()
    }
}

/// Integer.parseInt(): optional sign, at least one decimal digit, must fit an int.
pub fn java_parse_int(s: &str) -> Option<i32> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let digits = if b[0] == b'-' || b[0] == b'+' { &b[1..] } else { b };
    if digits.is_empty() || !digits.iter().all(|c| c.is_ascii_digit()) {
        // (Java also accepts non-ASCII Unicode digits; not relevant for index values)
        return None;
    }
    s.parse::<i32>().ok()
}

fn output_form_field(ofs: &mut dyn Write, ff: &FormField) -> std::io::Result<()> {
    writeln!(ofs, "---")?;
    writeln!(ofs, "FieldType: {}", ff.m_ft)?;
    writeln!(ofs, "FieldName: {}", ff.m_tt)?;
    if !ff.m_tu.is_empty() {
        writeln!(ofs, "FieldNameAlt: {}", ff.m_tu)?;
    }
    writeln!(ofs, "FieldFlags: {}", ff.m_ff)?;
    for it in &ff.m_vv.0 {
        writeln!(ofs, "FieldValue: {}", ff.interpret_value(it))?;
    }
    if !ff.m_dv.is_empty() {
        writeln!(ofs, "FieldValueDefault: {}", ff.interpret_value(&ff.m_dv))?;
    }
    write!(ofs, "FieldJustification: ")?;
    match ff.m_qq {
        0 => writeln!(ofs, "Left")?,
        1 => writeln!(ofs, "Center")?,
        2 => writeln!(ofs, "Right")?,
        q => writeln!(ofs, "{}", q)?,
    }
    if !ff.m_ds.is_empty() {
        writeln!(ofs, "FieldStyleDefault: {}", ff.m_ds)?;
    }
    if !ff.m_rv.is_empty() {
        write!(ofs, "FieldValueRichText: ")?;
        ofs.write_all(&ff.m_rv)?;
        writeln!(ofs)?;
    }
    if 0 < ff.m_maxlen {
        writeln!(ofs, "FieldMaxLength: {}", ff.m_maxlen)?;
    }
    for it in &ff.m_states.0 {
        writeln!(ofs, "FieldStateOption: {}", it)?;
    }
    for it in &ff.m_states_value_display.0 {
        writeln!(ofs, "FieldStateOption: {}", it[0])?;
        writeln!(ofs, "FieldStateOptionDisplay: {}", it[1])?;
    }
    Ok(())
}

fn report_kids(
    ofs: &mut dyn Write,
    kids: &[Object],
    acc_state: &mut FormField,
    doc: &Document,
    utf8_b: bool,
    depth: usize,
) -> std::io::Result<bool> {
    let prev_state = acc_state.clone();
    let mut ret_val_b = false;
    if depth > 200 {
        return Ok(false); // cyclic /Kids: the Java overflows its stack here
    }

    for kids_ii in kids {
        let kid_p = match res(doc, Some(kids_ii)) {
            Some(Object::Dictionary(d)) => d,
            _ => continue,
        };

        // field type
        if kid_p.has(b"FT") {
            if let Some(Object::Name(ft)) = res(doc, kid_p.get(b"FT").ok()) {
                match ft.as_slice() {
                    b"Btn" => acc_state.m_ft = "Button".into(),
                    b"Tx" => acc_state.m_ft = "Text".into(),
                    b"Ch" => acc_state.m_ft = "Choice".into(),
                    b"Sig" => acc_state.m_ft = "Signature".into(),
                    _ => eprintln!("pdftk Warning in ReportAcroFormFields(): unexpected field type;"),
                }
            }
        }

        // field name; special inheritance rule: prepend parent name
        if kid_p.has(b"T") {
            if let Some(o @ Object::String(..)) = res(doc, kid_p.get(b"T").ok()) {
                if !acc_state.m_tt.is_empty() {
                    acc_state.m_tt.push('.');
                }
                acc_state.m_tt.push_str(&report::output_pdf_string(Some(o), utf8_b));
            }
        }

        // field alt. name
        if kid_p.has(b"TU") {
            if let Some(o @ Object::String(..)) = res(doc, kid_p.get(b"TU").ok()) {
                acc_state.m_tu = report::output_pdf_string(Some(o), utf8_b);
            }
        } else {
            acc_state.m_tu.clear();
        }

        // field flags; inheritable
        if kid_p.has(b"Ff") {
            if let Some(o) = res(doc, kid_p.get(b"Ff").ok()) {
                if is_number(o) {
                    acc_state.m_ff = int_value(o);
                }
            }
        }

        // field value; inheritable; may be string or name
        if kid_p.has(b"V") {
            let v = match res(doc, kid_p.get(b"V").ok()) {
                Some(v) => v,
                None => continue, // sic
            };
            if let Some(s) = report::output_pdf_string_or_name(Some(v), utf8_b) {
                acc_state.m_vv.add(s);
            } else if let Object::Array(vv) = v {
                // multiple selections
                for vv_ii in vv {
                    if let Some(s) = report::output_pdf_string_or_name(res(doc, Some(vv_ii)), utf8_b) {
                        acc_state.m_vv.add(s);
                    }
                }
            }
        }

        // default value; inheritable
        if kid_p.has(b"DV") {
            if let Some(s) = report::output_pdf_string_or_name(res(doc, kid_p.get(b"DV").ok()), utf8_b) {
                acc_state.m_dv = s;
            }
        }

        // quadding; inheritable
        if kid_p.has(b"Q") {
            if let Some(o) = res(doc, kid_p.get(b"Q").ok()) {
                if is_number(o) {
                    acc_state.m_qq = int_value(o);
                }
            }
        }

        // default style
        if kid_p.has(b"DS") {
            if let Some(o @ Object::String(..)) = res(doc, kid_p.get(b"DS").ok()) {
                acc_state.m_ds = report::output_pdf_string(Some(o), utf8_b);
            }
        } else {
            acc_state.m_ds.clear();
        }

        // rich text value; may be a string or a stream
        if kid_p.has(b"RV") {
            match res(doc, kid_p.get(b"RV").ok()) {
                Some(o @ Object::String(..)) => {
                    acc_state.m_rv = report::output_pdf_string(Some(o), utf8_b).into_bytes();
                }
                Some(Object::Stream(s)) => {
                    // (PRStream.getBytes() in the Java; we emit the decoded stream data)
                    acc_state.m_rv = s.decompressed_content().unwrap_or_else(|_| s.content.clone());
                }
                _ => {}
            }
        } else {
            acc_state.m_rv.clear();
        }

        // maximum length; inheritable
        if kid_p.has(b"MaxLen") {
            if let Some(o) = res(doc, kid_p.get(b"MaxLen").ok()) {
                if is_number(o) {
                    acc_state.m_maxlen = int_value(o);
                }
            }
        }

        // list-box / combo-box possible states
        if kid_p.has(b"Opt") {
            if let Some(Object::Array(opts)) = res(doc, kid_p.get(b"Opt").ok()) {
                acc_state.m_have_option_list = true;
                acc_state.add_options(doc, opts, utf8_b);
            }
        }

        // available states
        if kid_p.has(b"AP") {
            if let Some(Object::Dictionary(ap)) = res(doc, kid_p.get(b"AP").ok()) {
                // this is one way to cull button option names: iterate over
                // appearance state names
                acc_state.add_ap_states(doc, ap, b"N", utf8_b);
                acc_state.add_ap_states(doc, ap, b"D", utf8_b);
                acc_state.add_ap_states(doc, ap, b"R", utf8_b);
            }
        }

        if kid_p.has(b"Kids") {
            // recurse
            if let Some(Object::Array(kid_kids)) = res(doc, kid_p.get(b"Kids").ok()) {
                let kids_have_names_b = report_kids(ofs, kid_kids, acc_state, doc, utf8_b, depth + 1)?;
                if !kids_have_names_b && kid_p.has(b"T") {
                    // dump form field
                    output_form_field(ofs, acc_state)?;
                }
                // reset state;
                *acc_state = prev_state.clone();
                // record field names found in subtree
                ret_val_b |= kids_have_names_b;
            }
        } else if kid_p.has(b"T") {
            // term. field; dump form field
            output_form_field(ofs, acc_state)?;
            // reset state;
            *acc_state = prev_state.clone();
            // record presense of field name
            ret_val_b = true;
        }
    }
    Ok(ret_val_b)
}

pub fn report_acro_form_fields(ofs: &mut dyn Write, doc: &Document, utf8_b: bool) -> std::io::Result<()> {
    let catalog = match doc.catalog() {
        Ok(c) => c,
        Err(_) => {
            eprintln!("pdftk Error in ReportAcroFormFields(): unable to access PDF catalog;");
            return Ok(());
        }
    };
    if let Some(Object::Dictionary(acro_form)) = res(doc, catalog.get(b"AcroForm").ok()) {
        if let Some(Object::Array(fields)) = res(doc, acro_form.get(b"Fields").ok()) {
            // enter recursion
            let mut root_field_state = FormField::new();
            report_kids(ofs, fields, &mut root_field_state, doc, utf8_b, 0)?;
        }
    }
    Ok(())
}
