//! Language of a mention: `whatlang` on the text, ISO 639-1 out, `None`
//! when the text is too short or the guess too weak. A copy of `feed`'s
//! detector (project rule: copy, don't share).

/// `text` should be the mention with handles and links stripped.
pub fn detect(text: &str) -> Option<String> {
    if text.chars().filter(|c| c.is_alphabetic()).count() < 12 {
        return None;
    }
    match whatlang::detect(text) {
        Some(info) if info.is_reliable() || info.confidence() > 0.3 => {
            iso639_1(info.lang()).map(str::to_string)
        }
        _ => None,
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
    fn detects_or_declines() {
        assert_eq!(
            detect("Is this real? Where did this story come from?").as_deref(),
            Some("en")
        );
        assert_eq!(
            detect("この話はどこから出てきたのか、本当のことなのか教えてほしい").as_deref(),
            Some("ja")
        );
        assert_eq!(detect("prove it"), None);
        assert_eq!(detect(""), None);
    }
}
