//! Language detection: feed hint first, `whatlang` on the text otherwise,
//! `und` when neither is usable. Output is ISO 639-1 (plan §4).

pub fn detect(hint: Option<&str>, text: &str) -> String {
    if let Some(h) = hint.filter(|h| h.len() == 2 && h.chars().all(|c| c.is_ascii_lowercase())) {
        return h.to_string();
    }
    if text.chars().filter(|c| c.is_alphabetic()).count() < 12 {
        return "und".into();
    }
    match whatlang::detect(text) {
        Some(info) if info.is_reliable() || info.confidence() > 0.3 => iso639_1(info.lang())
            .map(str::to_string)
            .unwrap_or_else(|| "und".into()),
        _ => "und".into(),
    }
}

fn iso639_1(l: whatlang::Lang) -> Option<&'static str> {
    use whatlang::Lang::*;
    Some(match l {
        Eng => "en",
        Spa => "es",
        Fra => "fr",
        Deu => "de",
        Ita => "it",
        Por => "pt",
        Rus => "ru",
        Jpn => "ja",
        Cmn => "zh",
        Ara => "ar",
        Nld => "nl",
        Swe => "sv",
        Nob => "no",
        Dan => "da",
        Fin => "fi",
        Pol => "pl",
        Ces => "cs",
        Slk => "sk",
        Hun => "hu",
        Ron => "ro",
        Bul => "bg",
        Ell => "el",
        Tur => "tr",
        Heb => "he",
        Hin => "hi",
        Ben => "bn",
        Urd => "ur",
        Pes => "fa",
        Ukr => "uk",
        Vie => "vi",
        Tha => "th",
        Kor => "ko",
        Ind => "id",
        Tgl => "tl",
        Cat => "ca",
        Hrv => "hr",
        Srp => "sr",
        Slv => "sl",
        Lit => "lt",
        Lav => "lv",
        Est => "et",
        Afr => "af",
        Aze => "az",
        Kat => "ka",
        Hye => "hy",
        Bel => "be",
        Mkd => "mk",
        Tam => "ta",
        Tel => "te",
        Mar => "mr",
        Guj => "gu",
        Kan => "kn",
        Mal => "ml",
        Pan => "pa",
        Amh => "am",
        Epo => "eo",
        Lat => "la",
        Tuk => "tk",
        Uzb => "uz",
        Sin => "si",
        Khm => "km",
        Mya => "my",
        Nep => "ne",
        Yid => "yi",
        Jav => "jv",
        Zul => "zu",
        Sna => "sn",
        Aka => "ak",
        Ori => "or",
        Cym => "cy",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_wins_then_detection_then_und() {
        assert_eq!(detect(Some("fr"), "whatever"), "fr");
        assert_eq!(detect(Some("EN-gb"), "short"), "und");
        assert_eq!(
            detect(
                None,
                "The prime minister announced sweeping changes to the national budget this morning"
            ),
            "en"
        );
        assert_eq!(
            detect(
                None,
                "El presidente anunció cambios importantes en el presupuesto nacional esta mañana"
            ),
            "es"
        );
        assert_eq!(detect(None, "ok"), "und");
    }
}
