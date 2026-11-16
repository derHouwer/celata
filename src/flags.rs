// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Country flags from flag-icons 7.6.0, MIT, assets/flags/LICENSE.
//!
//! Slint can load images only through fixed paths. Rust therefore embeds the flags here, and
//! `src/main.rs` turns them into images with `slint::Image::load_from_svg_data`.

/// SVG data of the flag (4 : 3) for the country code in upper case, `None` for unknown codes.
pub fn svg(code: &str) -> Option<&'static [u8]> {
    Some(match code {
        "AE" => include_bytes!("../assets/flags/ae.svg"),
        "AL" => include_bytes!("../assets/flags/al.svg"),
        "AM" => include_bytes!("../assets/flags/am.svg"),
        "AR" => include_bytes!("../assets/flags/ar.svg"),
        "AT" => include_bytes!("../assets/flags/at.svg"),
        "AU" => include_bytes!("../assets/flags/au.svg"),
        "AZ" => include_bytes!("../assets/flags/az.svg"),
        "BA" => include_bytes!("../assets/flags/ba.svg"),
        "BD" => include_bytes!("../assets/flags/bd.svg"),
        "BE" => include_bytes!("../assets/flags/be.svg"),
        "BG" => include_bytes!("../assets/flags/bg.svg"),
        "BO" => include_bytes!("../assets/flags/bo.svg"),
        "BR" => include_bytes!("../assets/flags/br.svg"),
        "BY" => include_bytes!("../assets/flags/by.svg"),
        "CA" => include_bytes!("../assets/flags/ca.svg"),
        "CH" => include_bytes!("../assets/flags/ch.svg"),
        "CL" => include_bytes!("../assets/flags/cl.svg"),
        "CN" => include_bytes!("../assets/flags/cn.svg"),
        "CO" => include_bytes!("../assets/flags/co.svg"),
        "CR" => include_bytes!("../assets/flags/cr.svg"),
        "CY" => include_bytes!("../assets/flags/cy.svg"),
        "CZ" => include_bytes!("../assets/flags/cz.svg"),
        "DE" => include_bytes!("../assets/flags/de.svg"),
        "DK" => include_bytes!("../assets/flags/dk.svg"),
        "EC" => include_bytes!("../assets/flags/ec.svg"),
        "EE" => include_bytes!("../assets/flags/ee.svg"),
        "EG" => include_bytes!("../assets/flags/eg.svg"),
        "ES" => include_bytes!("../assets/flags/es.svg"),
        "FI" => include_bytes!("../assets/flags/fi.svg"),
        "FR" => include_bytes!("../assets/flags/fr.svg"),
        "GB" => include_bytes!("../assets/flags/gb.svg"),
        "GE" => include_bytes!("../assets/flags/ge.svg"),
        "GR" => include_bytes!("../assets/flags/gr.svg"),
        "HK" => include_bytes!("../assets/flags/hk.svg"),
        "HR" => include_bytes!("../assets/flags/hr.svg"),
        "HU" => include_bytes!("../assets/flags/hu.svg"),
        "ID" => include_bytes!("../assets/flags/id.svg"),
        "IE" => include_bytes!("../assets/flags/ie.svg"),
        "IL" => include_bytes!("../assets/flags/il.svg"),
        "IN" => include_bytes!("../assets/flags/in.svg"),
        "IS" => include_bytes!("../assets/flags/is.svg"),
        "IT" => include_bytes!("../assets/flags/it.svg"),
        "JP" => include_bytes!("../assets/flags/jp.svg"),
        "KE" => include_bytes!("../assets/flags/ke.svg"),
        "KH" => include_bytes!("../assets/flags/kh.svg"),
        "KR" => include_bytes!("../assets/flags/kr.svg"),
        "KZ" => include_bytes!("../assets/flags/kz.svg"),
        "LK" => include_bytes!("../assets/flags/lk.svg"),
        "LT" => include_bytes!("../assets/flags/lt.svg"),
        "LU" => include_bytes!("../assets/flags/lu.svg"),
        "LV" => include_bytes!("../assets/flags/lv.svg"),
        "MA" => include_bytes!("../assets/flags/ma.svg"),
        "MD" => include_bytes!("../assets/flags/md.svg"),
        "ME" => include_bytes!("../assets/flags/me.svg"),
        "MK" => include_bytes!("../assets/flags/mk.svg"),
        "MT" => include_bytes!("../assets/flags/mt.svg"),
        "MX" => include_bytes!("../assets/flags/mx.svg"),
        "MY" => include_bytes!("../assets/flags/my.svg"),
        "NG" => include_bytes!("../assets/flags/ng.svg"),
        "NL" => include_bytes!("../assets/flags/nl.svg"),
        "NO" => include_bytes!("../assets/flags/no.svg"),
        "NP" => include_bytes!("../assets/flags/np.svg"),
        "NZ" => include_bytes!("../assets/flags/nz.svg"),
        "PA" => include_bytes!("../assets/flags/pa.svg"),
        "PE" => include_bytes!("../assets/flags/pe.svg"),
        "PH" => include_bytes!("../assets/flags/ph.svg"),
        "PK" => include_bytes!("../assets/flags/pk.svg"),
        "PL" => include_bytes!("../assets/flags/pl.svg"),
        "PT" => include_bytes!("../assets/flags/pt.svg"),
        "QA" => include_bytes!("../assets/flags/qa.svg"),
        "RO" => include_bytes!("../assets/flags/ro.svg"),
        "RS" => include_bytes!("../assets/flags/rs.svg"),
        "SA" => include_bytes!("../assets/flags/sa.svg"),
        "SE" => include_bytes!("../assets/flags/se.svg"),
        "SG" => include_bytes!("../assets/flags/sg.svg"),
        "SI" => include_bytes!("../assets/flags/si.svg"),
        "SK" => include_bytes!("../assets/flags/sk.svg"),
        "TH" => include_bytes!("../assets/flags/th.svg"),
        "TR" => include_bytes!("../assets/flags/tr.svg"),
        "TW" => include_bytes!("../assets/flags/tw.svg"),
        "UA" => include_bytes!("../assets/flags/ua.svg"),
        "US" => include_bytes!("../assets/flags/us.svg"),
        "UY" => include_bytes!("../assets/flags/uy.svg"),
        "VN" => include_bytes!("../assets/flags/vn.svg"),
        "ZA" => include_bytes!("../assets/flags/za.svg"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names;

    #[test]
    fn jedes_bekannte_land_hat_eine_flagge() {
        for code in names::CODES {
            assert!(svg(code).is_some(), "Flagge fehlt für {code}");
        }
    }

    #[test]
    fn jede_flaggendatei_gehoert_zu_einem_land() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/flags");
        for entry in std::fs::read_dir(dir).expect("assets/flags fehlt") {
            let name = entry
                .expect("Eintrag lesbar")
                .file_name()
                .to_string_lossy()
                .into_owned();
            if name == "LICENSE" {
                continue;
            }
            let code = name
                .strip_suffix(".svg")
                .unwrap_or_else(|| panic!("unerwartete Datei {name}"))
                .to_uppercase();
            assert!(names::is_known(&code), "Datei {name} gehört zu keinem bekannten Land");
        }
    }

    #[test]
    fn jede_flagge_laesst_sich_laden() {
        for code in names::CODES {
            let data = svg(code).expect("Flagge vorhanden");
            let image = slint::Image::load_from_svg_data(data).unwrap_or_else(|_| panic!("Flagge {code} nicht ladbar"));
            let size = image.size();
            assert!(size.width > 0 && size.height > 0, "Flagge {code} ohne Größe");
        }
    }

    #[test]
    fn unbekanntes_land_hat_keine_flagge() {
        assert_eq!(svg("XX"), None);
        assert_eq!(svg("de"), None);
    }
}
