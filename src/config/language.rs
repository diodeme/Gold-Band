use super::DesktopLanguage;

impl DesktopLanguage {
    pub const ALL: [Self; 7] = [
        Self::ZhCn,
        Self::ZhTw,
        Self::En,
        Self::JaJp,
        Self::KoKr,
        Self::PtBr,
        Self::Es,
    ];

    /// Map a BCP 47 tag onto the supported catalog. Unmatched tags become English.
    /// Explicit `Hans`/`Hant` scripts take precedence over Chinese regions.
    /// Without a script, Taiwan, Hong Kong, and Macau use Traditional Chinese;
    /// other Chinese tags use Simplified Chinese. Portuguese matches only `pt-BR`.
    pub fn from_locale_tag(tag: &str) -> Self {
        let parts = tag.trim().replace('_', "-").to_ascii_lowercase();
        let mut parts = parts.split('-').filter(|part| !part.is_empty());
        let primary = parts.next().unwrap_or("");
        let rest = parts.collect::<Vec<_>>();
        match primary {
            "zh" => {
                let script = rest
                    .iter()
                    .copied()
                    .find(|part| matches!(*part, "hans" | "hant"));
                match script {
                    Some("hans") => Self::ZhCn,
                    Some("hant") => Self::ZhTw,
                    _ if rest.iter().any(|part| matches!(*part, "tw" | "hk" | "mo")) => Self::ZhTw,
                    _ => Self::ZhCn,
                }
            }
            "ja" => Self::JaJp,
            "ko" => Self::KoKr,
            "pt" => {
                if rest.iter().any(|part| *part == "br") {
                    Self::PtBr
                } else {
                    Self::En
                }
            }
            "es" => Self::Es,
            "en" => Self::En,
            _ => Self::En,
        }
    }

    /// Map a Windows UI language id. This follows `GetUserDefaultUILanguage`,
    /// which is the same source NSIS uses for the installer language.
    pub fn from_windows_langid(langid: u16) -> Self {
        const LANG_CHINESE: u16 = 0x04;
        const LANG_ENGLISH: u16 = 0x09;
        const LANG_SPANISH: u16 = 0x0a;
        const LANG_JAPANESE: u16 = 0x11;
        const LANG_KOREAN: u16 = 0x12;
        const LANG_PORTUGUESE: u16 = 0x16;
        const SUBLANG_CHINESE_TRADITIONAL: u16 = 0x01;
        const SUBLANG_CHINESE_HONGKONG: u16 = 0x03;
        const SUBLANG_CHINESE_MACAU: u16 = 0x05;
        const SUBLANG_PORTUGUESE_BRAZILIAN: u16 = 0x01;

        let primary = langid & 0x3ff;
        let sublang = langid >> 10;
        match primary {
            LANG_CHINESE => match sublang {
                SUBLANG_CHINESE_TRADITIONAL | SUBLANG_CHINESE_HONGKONG | SUBLANG_CHINESE_MACAU => {
                    Self::ZhTw
                }
                _ => Self::ZhCn,
            },
            LANG_JAPANESE => Self::JaJp,
            LANG_KOREAN => Self::KoKr,
            LANG_PORTUGUESE => {
                if sublang == SUBLANG_PORTUGUESE_BRAZILIAN {
                    Self::PtBr
                } else {
                    Self::En
                }
            }
            LANG_SPANISH => Self::Es,
            LANG_ENGLISH => Self::En,
            _ => Self::En,
        }
    }

    pub fn from_system() -> Self {
        #[cfg(windows)]
        {
            Self::from_windows_langid(unsafe { GetUserDefaultUILanguage() })
        }
        #[cfg(not(windows))]
        {
            sys_locale::get_locale()
                .as_deref()
                .map(Self::from_locale_tag)
                .unwrap_or(Self::En)
        }
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetUserDefaultUILanguage() -> u16;
}

#[cfg(test)]
mod tests {
    use super::DesktopLanguage;

    #[test]
    fn explicit_chinese_script_takes_precedence_over_region() {
        for (tag, expected) in [
            ("zh-Hans", DesktopLanguage::ZhCn),
            ("zh-Hans-CN", DesktopLanguage::ZhCn),
            ("zh-Hans-SG", DesktopLanguage::ZhCn),
            ("zh-Hans-HK", DesktopLanguage::ZhCn),
            ("zh-Hans-TW", DesktopLanguage::ZhCn),
            ("zh-Hans-MO", DesktopLanguage::ZhCn),
            ("zh-Hant", DesktopLanguage::ZhTw),
            ("zh-Hant-CN", DesktopLanguage::ZhTw),
            ("zh-Hant-SG", DesktopLanguage::ZhTw),
            ("zh-Hant-HK", DesktopLanguage::ZhTw),
            (" ZH_hAnS_hK ", DesktopLanguage::ZhCn),
        ] {
            assert_eq!(DesktopLanguage::from_locale_tag(tag), expected, "{tag}");
        }
    }

    #[test]
    fn chinese_region_is_used_when_script_is_absent() {
        for (tag, expected) in [
            ("zh", DesktopLanguage::ZhCn),
            ("zh-CN", DesktopLanguage::ZhCn),
            ("zh-SG", DesktopLanguage::ZhCn),
            ("zh-TW", DesktopLanguage::ZhTw),
            ("zh-HK", DesktopLanguage::ZhTw),
            ("zh-MO", DesktopLanguage::ZhTw),
        ] {
            assert_eq!(DesktopLanguage::from_locale_tag(tag), expected, "{tag}");
        }
    }

    #[test]
    fn locale_tags_map_onto_the_supported_catalog() {
        assert_eq!(
            DesktopLanguage::from_locale_tag("zh-CN"),
            DesktopLanguage::ZhCn
        );
        assert_eq!(
            DesktopLanguage::from_locale_tag("zh-Hans-CN"),
            DesktopLanguage::ZhCn
        );
        assert_eq!(
            DesktopLanguage::from_locale_tag("zh"),
            DesktopLanguage::ZhCn
        );
        assert_eq!(
            DesktopLanguage::from_locale_tag("zh_TW"),
            DesktopLanguage::ZhTw
        );
        assert_eq!(
            DesktopLanguage::from_locale_tag("zh-Hant-HK"),
            DesktopLanguage::ZhTw
        );
        assert_eq!(
            DesktopLanguage::from_locale_tag("zh-MO"),
            DesktopLanguage::ZhTw
        );
        assert_eq!(
            DesktopLanguage::from_locale_tag("en-US"),
            DesktopLanguage::En
        );
        assert_eq!(
            DesktopLanguage::from_locale_tag("ja-JP"),
            DesktopLanguage::JaJp
        );
        assert_eq!(
            DesktopLanguage::from_locale_tag("ko-KR"),
            DesktopLanguage::KoKr
        );
        assert_eq!(
            DesktopLanguage::from_locale_tag("pt-BR"),
            DesktopLanguage::PtBr
        );
        assert_eq!(
            DesktopLanguage::from_locale_tag("pt-PT"),
            DesktopLanguage::En
        );
        assert_eq!(
            DesktopLanguage::from_locale_tag("es-MX"),
            DesktopLanguage::Es
        );
        assert_eq!(
            DesktopLanguage::from_locale_tag("de-DE"),
            DesktopLanguage::En
        );
    }

    #[test]
    fn windows_language_ids_match_the_installer_catalog() {
        assert_eq!(
            DesktopLanguage::from_windows_langid(2052),
            DesktopLanguage::ZhCn
        );
        assert_eq!(
            DesktopLanguage::from_windows_langid(1028),
            DesktopLanguage::ZhTw
        );
        assert_eq!(
            DesktopLanguage::from_windows_langid(3076),
            DesktopLanguage::ZhTw
        );
        assert_eq!(
            DesktopLanguage::from_windows_langid(1033),
            DesktopLanguage::En
        );
        assert_eq!(
            DesktopLanguage::from_windows_langid(1041),
            DesktopLanguage::JaJp
        );
        assert_eq!(
            DesktopLanguage::from_windows_langid(1042),
            DesktopLanguage::KoKr
        );
        assert_eq!(
            DesktopLanguage::from_windows_langid(1046),
            DesktopLanguage::PtBr
        );
        assert_eq!(
            DesktopLanguage::from_windows_langid(2070),
            DesktopLanguage::En
        );
        assert_eq!(
            DesktopLanguage::from_windows_langid(1034),
            DesktopLanguage::Es
        );
        assert_eq!(
            DesktopLanguage::from_windows_langid(2058),
            DesktopLanguage::Es
        );
        assert_eq!(
            DesktopLanguage::from_windows_langid(1031),
            DesktopLanguage::En
        );
    }

    #[test]
    fn persisted_language_values_round_trip() {
        for value in ["zh-cn", "zh-tw", "en", "ja-jp", "ko-kr", "pt-br", "es"] {
            let language: DesktopLanguage = value.parse().unwrap();
            assert_eq!(
                serde_json::to_value(language).unwrap(),
                serde_json::Value::String(value.to_string())
            );
        }
        assert!("fr".parse::<DesktopLanguage>().is_err());
    }
}
