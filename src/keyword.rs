//! Port of keyword.java. Owner: CLI agent.
#![allow(non_camel_case_types, dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Keyword {
  none_k,
  cat_k,
  shuffle_k,
  burst_k,
  barcode_burst_k,
  filter_k,
  dump_data_k,
  dump_data_utf8_k,
  dump_data_fields_k,
  dump_data_fields_utf8_k,
  dump_data_annots_k,
  generate_fdf_k,
  unpack_files_k,
  fill_form_k,
  attach_file_k,
  update_info_k,
  update_info_utf8_k,
  background_k,
  multibackground_k,
  stamp_k,
  multistamp_k,
  rotate_k,
  attach_file_to_page_k,
  attach_file_relation_k,
  even_k,
  odd_k,
  range_minus_k,
  output_k,
  input_pw_k,
  owner_pw_k,
  user_pw_k,
  user_perms_k,
  encrypt_40bit_k,
  encrypt_128bit_k,
  encrypt_aes128_k,
  perm_printing_k,
  perm_modify_contents_k,
  perm_copy_contents_k,
  perm_modify_annotations_k,
  perm_fillin_k,
  perm_screen_readers_k,
  perm_assembly_k,
  perm_degraded_printing_k,
  perm_all_k,
  filt_uncompress_k,
  filt_compress_k,
  flatten_k,
  need_appearances_k,
  drop_xfa_k,
  drop_xmp_k,
  keep_first_id_k,
  keep_final_id_k,
  replacement_font_k,
  verbose_k,
  dont_ask_k,
  do_ask_k,
  rot_north_k,
  rot_east_k,
  rot_south_k,
  rot_west_k,
  rot_left_k,
  rot_right_k,
  rot_upside_down_k,
}

impl Keyword {
    /// All keywords, in declaration order (used for the `valueOf(ss + "_k")` lookup).
    const ALL: [Keyword; 63] = [
        Keyword::none_k,
        Keyword::cat_k,
        Keyword::shuffle_k,
        Keyword::burst_k,
        Keyword::barcode_burst_k,
        Keyword::filter_k,
        Keyword::dump_data_k,
        Keyword::dump_data_utf8_k,
        Keyword::dump_data_fields_k,
        Keyword::dump_data_fields_utf8_k,
        Keyword::dump_data_annots_k,
        Keyword::generate_fdf_k,
        Keyword::unpack_files_k,
        Keyword::fill_form_k,
        Keyword::attach_file_k,
        Keyword::update_info_k,
        Keyword::update_info_utf8_k,
        Keyword::background_k,
        Keyword::multibackground_k,
        Keyword::stamp_k,
        Keyword::multistamp_k,
        Keyword::rotate_k,
        Keyword::attach_file_to_page_k,
        Keyword::attach_file_relation_k,
        Keyword::even_k,
        Keyword::odd_k,
        Keyword::range_minus_k,
        Keyword::output_k,
        Keyword::input_pw_k,
        Keyword::owner_pw_k,
        Keyword::user_pw_k,
        Keyword::user_perms_k,
        Keyword::encrypt_40bit_k,
        Keyword::encrypt_128bit_k,
        Keyword::encrypt_aes128_k,
        Keyword::perm_printing_k,
        Keyword::perm_modify_contents_k,
        Keyword::perm_copy_contents_k,
        Keyword::perm_modify_annotations_k,
        Keyword::perm_fillin_k,
        Keyword::perm_screen_readers_k,
        Keyword::perm_assembly_k,
        Keyword::perm_degraded_printing_k,
        Keyword::perm_all_k,
        Keyword::filt_uncompress_k,
        Keyword::filt_compress_k,
        Keyword::flatten_k,
        Keyword::need_appearances_k,
        Keyword::drop_xfa_k,
        Keyword::drop_xmp_k,
        Keyword::keep_first_id_k,
        Keyword::keep_final_id_k,
        Keyword::replacement_font_k,
        Keyword::verbose_k,
        Keyword::dont_ask_k,
        Keyword::do_ask_k,
        Keyword::rot_north_k,
        Keyword::rot_east_k,
        Keyword::rot_south_k,
        Keyword::rot_west_k,
        Keyword::rot_left_k,
        Keyword::rot_right_k,
        Keyword::rot_upside_down_k,
    ];

    /// The Java enum constant name, e.g. "cat_k".
    pub fn name(self) -> &'static str {
        match self {
            Keyword::none_k => "none_k",
            Keyword::cat_k => "cat_k",
            Keyword::shuffle_k => "shuffle_k",
            Keyword::burst_k => "burst_k",
            Keyword::barcode_burst_k => "barcode_burst_k",
            Keyword::filter_k => "filter_k",
            Keyword::dump_data_k => "dump_data_k",
            Keyword::dump_data_utf8_k => "dump_data_utf8_k",
            Keyword::dump_data_fields_k => "dump_data_fields_k",
            Keyword::dump_data_fields_utf8_k => "dump_data_fields_utf8_k",
            Keyword::dump_data_annots_k => "dump_data_annots_k",
            Keyword::generate_fdf_k => "generate_fdf_k",
            Keyword::unpack_files_k => "unpack_files_k",
            Keyword::fill_form_k => "fill_form_k",
            Keyword::attach_file_k => "attach_file_k",
            Keyword::update_info_k => "update_info_k",
            Keyword::update_info_utf8_k => "update_info_utf8_k",
            Keyword::background_k => "background_k",
            Keyword::multibackground_k => "multibackground_k",
            Keyword::stamp_k => "stamp_k",
            Keyword::multistamp_k => "multistamp_k",
            Keyword::rotate_k => "rotate_k",
            Keyword::attach_file_to_page_k => "attach_file_to_page_k",
            Keyword::attach_file_relation_k => "attach_file_relation_k",
            Keyword::even_k => "even_k",
            Keyword::odd_k => "odd_k",
            Keyword::range_minus_k => "range_minus_k",
            Keyword::output_k => "output_k",
            Keyword::input_pw_k => "input_pw_k",
            Keyword::owner_pw_k => "owner_pw_k",
            Keyword::user_pw_k => "user_pw_k",
            Keyword::user_perms_k => "user_perms_k",
            Keyword::encrypt_40bit_k => "encrypt_40bit_k",
            Keyword::encrypt_128bit_k => "encrypt_128bit_k",
            Keyword::encrypt_aes128_k => "encrypt_aes128_k",
            Keyword::perm_printing_k => "perm_printing_k",
            Keyword::perm_modify_contents_k => "perm_modify_contents_k",
            Keyword::perm_copy_contents_k => "perm_copy_contents_k",
            Keyword::perm_modify_annotations_k => "perm_modify_annotations_k",
            Keyword::perm_fillin_k => "perm_fillin_k",
            Keyword::perm_screen_readers_k => "perm_screen_readers_k",
            Keyword::perm_assembly_k => "perm_assembly_k",
            Keyword::perm_degraded_printing_k => "perm_degraded_printing_k",
            Keyword::perm_all_k => "perm_all_k",
            Keyword::filt_uncompress_k => "filt_uncompress_k",
            Keyword::filt_compress_k => "filt_compress_k",
            Keyword::flatten_k => "flatten_k",
            Keyword::need_appearances_k => "need_appearances_k",
            Keyword::drop_xfa_k => "drop_xfa_k",
            Keyword::drop_xmp_k => "drop_xmp_k",
            Keyword::keep_first_id_k => "keep_first_id_k",
            Keyword::keep_final_id_k => "keep_final_id_k",
            Keyword::replacement_font_k => "replacement_font_k",
            Keyword::verbose_k => "verbose_k",
            Keyword::dont_ask_k => "dont_ask_k",
            Keyword::do_ask_k => "do_ask_k",
            Keyword::rot_north_k => "rot_north_k",
            Keyword::rot_east_k => "rot_east_k",
            Keyword::rot_south_k => "rot_south_k",
            Keyword::rot_west_k => "rot_west_k",
            Keyword::rot_left_k => "rot_left_k",
            Keyword::rot_right_k => "rot_right_k",
            Keyword::rot_upside_down_k => "rot_upside_down_k",
        }
    }

    /// Port of keyword.is_keyword(). Returns Keyword::none_k if `ss` is not a keyword.
    pub fn is_keyword(ss: &str) -> Keyword {
        use Keyword::*;
        let ss = ss.to_lowercase();
        let ss = ss.as_str();

        // Java: keyword.valueOf(ss + "_k")
        for k in Self::ALL {
            let n = k.name();
            if n.len() == ss.len() + 2 && n.starts_with(ss) {
                return k;
            }
        }

        match ss {
            // operations
            "dumpdata" | "data_dump" | "datadump" => return dump_data_k,
            "fdfgen" | "fdfdump" | "dump_data_fields_fdf" => return generate_fdf_k,
            "fillform" => return fill_form_k,
            "attach_files" | "attachfile" => return attach_file_k,
            "unpack_file" | "unpackfiles" => return unpack_files_k,
            "updateinfo" => return update_info_k,
            "updateinfoutf8" => return update_info_utf8_k,
            _ => {}
        }

        // cat range keywords (note: prefix match, as strncmp in the original)
        if ss.starts_with("even") {
            return even_k;
        } else if ss.starts_with("odd") {
            return odd_k;
        }

        match ss {
            // file attachment option
            "to_page" | "topage" => attach_file_to_page_k,
            "relation" => attach_file_relation_k,

            // encryption & decryption; depends on context
            "ownerpw" => owner_pw_k,
            "userpw" => user_pw_k,
            "inputpw" => input_pw_k,
            "allow" => user_perms_k,

            // expect these only in output section
            "encrypt_40bits" | "encrypt40bit" | "encrypt40bits" | "encrypt40_bit"
            | "encrypt40_bits" | "encrypt_40_bit" | "encrypt_40_bits" => encrypt_40bit_k,
            "encrypt_128bits" | "encrypt128bit" | "encrypt128bits" | "encrypt128_bit"
            | "encrypt128_bits" | "encrypt_128_bit" | "encrypt_128_bits" => encrypt_128bit_k,

            // user permissions; must follow user_perms_k
            "printing" => perm_printing_k,
            "modifycontents" => perm_modify_contents_k,
            "copycontents" => perm_copy_contents_k,
            "modifyannotations" => perm_modify_annotations_k,
            "fillin" => perm_fillin_k,
            "screenreaders" => perm_screen_readers_k,
            "assembly" => perm_assembly_k,
            "degradedprinting" => perm_degraded_printing_k,
            "allfeatures" => perm_all_k,

            // pdftk keywords
            "uncompress" => filt_uncompress_k,
            "compress" => filt_compress_k,
            "dontask" => dont_ask_k,

            // more cat range keywords
            "north" => rot_north_k,
            "south" => rot_south_k,
            "east" => rot_east_k,
            "west" => rot_west_k,
            "left" => rot_left_k,
            "right" => rot_right_k,
            "down" => rot_upside_down_k,
            "~" => range_minus_k,

            _ => none_k,
        }
    }

    /// Port of keyword.consume_keyword(): consumes a keyword from the front of
    /// `ssb` (the whole remainder unless it starts with even/odd/~).
    pub fn consume_keyword(ssb: &mut String) -> Keyword {
        let ss = ssb.to_lowercase();
        // Byte offsets below are into the original string; the matched prefixes
        // are ASCII, so (barring exotic case mappings) they line up.
        let cut = |ssb: &mut String, n: usize| {
            let mut n = n.min(ssb.len());
            while !ssb.is_char_boundary(n) {
                n += 1;
            }
            ssb.drain(..n);
        };
        if ss.starts_with("even") {
            cut(ssb, 4);
            Keyword::even_k
        } else if ss.starts_with("odd") {
            cut(ssb, 3);
            Keyword::odd_k
        } else if ss.starts_with('~') {
            cut(ssb, 1);
            Keyword::range_minus_k
        } else {
            ssb.clear();
            Self::is_keyword(&ss)
        }
    }
}

impl std::fmt::Display for Keyword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::Keyword::{self, *};

    #[test]
    fn enum_names() {
        assert_eq!(Keyword::is_keyword("cat"), cat_k);
        assert_eq!(Keyword::is_keyword("CAT"), cat_k);
        assert_eq!(Keyword::is_keyword("Dump_Data_Utf8"), dump_data_utf8_k);
        assert_eq!(Keyword::is_keyword("output"), output_k);
        assert_eq!(Keyword::is_keyword("input_pw"), input_pw_k);
        assert_eq!(Keyword::is_keyword("encrypt_aes128"), encrypt_aes128_k);
        assert_eq!(Keyword::is_keyword("replacement_font"), replacement_font_k);
        // quirks of valueOf(ss + "_k")
        assert_eq!(Keyword::is_keyword("rot_north"), rot_north_k);
        assert_eq!(Keyword::is_keyword("filt_compress"), filt_compress_k);
        assert_eq!(Keyword::is_keyword("none"), none_k);
        for k in Keyword::ALL {
            let n = k.name();
            assert_eq!(Keyword::is_keyword(&n[..n.len() - 2]), k);
            assert_eq!(format!("{:?}", k), n);
        }
    }

    #[test]
    fn aliases() {
        let cases: &[(&str, Keyword)] = &[
            ("dumpdata", dump_data_k), ("data_dump", dump_data_k), ("datadump", dump_data_k),
            ("fdfgen", generate_fdf_k), ("fdfdump", generate_fdf_k),
            ("dump_data_fields_fdf", generate_fdf_k),
            ("fillform", fill_form_k),
            ("attach_files", attach_file_k), ("attachfile", attach_file_k),
            ("unpack_file", unpack_files_k), ("unpackfiles", unpack_files_k),
            ("updateinfo", update_info_k), ("updateinfoutf8", update_info_utf8_k),
            ("even", even_k), ("evenwest", even_k), ("odd", odd_k), ("oddity.pdf", odd_k),
            ("to_page", attach_file_to_page_k), ("topage", attach_file_to_page_k),
            ("relation", attach_file_relation_k),
            ("ownerpw", owner_pw_k), ("userpw", user_pw_k), ("inputpw", input_pw_k),
            ("allow", user_perms_k),
            ("encrypt_40bits", encrypt_40bit_k), ("encrypt40bit", encrypt_40bit_k),
            ("encrypt40bits", encrypt_40bit_k), ("encrypt40_bit", encrypt_40bit_k),
            ("encrypt40_bits", encrypt_40bit_k), ("encrypt_40_bit", encrypt_40bit_k),
            ("encrypt_40_bits", encrypt_40bit_k),
            ("encrypt_128bits", encrypt_128bit_k), ("encrypt128bit", encrypt_128bit_k),
            ("encrypt128bits", encrypt_128bit_k), ("encrypt128_bit", encrypt_128bit_k),
            ("encrypt128_bits", encrypt_128bit_k), ("encrypt_128_bit", encrypt_128bit_k),
            ("encrypt_128_bits", encrypt_128bit_k),
            ("printing", perm_printing_k), ("ModifyContents", perm_modify_contents_k),
            ("CopyContents", perm_copy_contents_k),
            ("ModifyAnnotations", perm_modify_annotations_k), ("FillIn", perm_fillin_k),
            ("ScreenReaders", perm_screen_readers_k), ("Assembly", perm_assembly_k),
            ("DegradedPrinting", perm_degraded_printing_k), ("AllFeatures", perm_all_k),
            ("uncompress", filt_uncompress_k), ("compress", filt_compress_k),
            ("dontask", dont_ask_k), ("dont_ask", dont_ask_k), ("do_ask", do_ask_k),
            ("north", rot_north_k), ("south", rot_south_k), ("east", rot_east_k),
            ("west", rot_west_k), ("left", rot_left_k), ("right", rot_right_k),
            ("down", rot_upside_down_k), ("~", range_minus_k),
            ("", none_k), ("foo.pdf", none_k), ("A=foo.pdf", none_k), ("1-end", none_k),
            ("doask", none_k), ("encrypt_aes_128", none_k), ("up", none_k),
        ];
        for (s, k) in cases {
            assert_eq!(Keyword::is_keyword(s), *k, "{}", s);
        }
    }

    #[test]
    fn consume() {
        let mut s = String::from("evenwest");
        assert_eq!(Keyword::consume_keyword(&mut s), even_k);
        assert_eq!(s, "west");
        assert_eq!(Keyword::consume_keyword(&mut s), rot_west_k);
        assert_eq!(s, "");
        let mut s = String::from("ODDdown");
        assert_eq!(Keyword::consume_keyword(&mut s), odd_k);
        assert_eq!(s, "down");
        let mut s = String::from("~3-5east");
        assert_eq!(Keyword::consume_keyword(&mut s), range_minus_k);
        assert_eq!(s, "3-5east");
        let mut s = String::from("westeven");
        assert_eq!(Keyword::consume_keyword(&mut s), none_k);
        assert_eq!(s, "");
    }
}
