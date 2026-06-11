use std::fmt::Display;

/// A supported language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// Let the model detect the spoken language itself.
    Auto,

    /// Generic arabic.
    Arabic,

    /// The dutch language.
    Dutch,

    /// Generic english.
    English,

    /// Generic french.
    French,

    /// Generic german.
    German,

    /// The greek language.
    Greek,

    /// The italian language.
    Italian,

    /// The japanese language.
    Japanese,

    /// The korean language.
    Korean,

    /// A chinese (mandarin) language.
    Mandarin,

    /// The polish language.
    Polish,

    /// Generic portuguese.
    Portuguese,

    /// Generic spanish.
    Spanish,

    /// The vietnamese language.
    Vietnamese,
}

impl Language {
    /// A list with all the supported languages.
    pub const ALL: &'static [Self] = &[
        Self::Auto,
        Self::Arabic,
        Self::Dutch,
        Self::English,
        Self::French,
        Self::German,
        Self::Greek,
        Self::Italian,
        Self::Japanese,
        Self::Korean,
        Self::Mandarin,
        Self::Polish,
        Self::Portuguese,
        Self::Spanish,
        Self::Vietnamese,
    ];

    /// The language code understood by the Nemotron 3.5 multilingual model
    /// (a key of its prompt dictionary).
    pub fn code(&self) -> &'static str {
        match self {
            Language::Auto => "auto",
            Language::Arabic => "ar",
            Language::Dutch => "nl",
            Language::English => "en",
            Language::French => "fr",
            Language::German => "de",
            Language::Greek => "el",
            Language::Italian => "it",
            Language::Japanese => "ja-JP",
            Language::Korean => "ko-KR",
            Language::Mandarin => "zh-CN",
            Language::Polish => "pl",
            Language::Portuguese => "pt",
            Language::Spanish => "es",
            Language::Vietnamese => "vi-VN",
        }
    }
}

impl Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Language::Auto => f.write_str("Auto"),
            Language::Arabic => f.write_str("Arabic"),
            Language::Dutch => f.write_str("Dutch"),
            Language::English => f.write_str("English"),
            Language::French => f.write_str("French"),
            Language::German => f.write_str("German"),
            Language::Greek => f.write_str("Greek"),
            Language::Italian => f.write_str("Italian"),
            Language::Japanese => f.write_str("Japanese"),
            Language::Korean => f.write_str("Korean"),
            Language::Mandarin => f.write_str("Mandarin"),
            Language::Polish => f.write_str("Polish"),
            Language::Portuguese => f.write_str("Portuguese"),
            Language::Spanish => f.write_str("Spanish"),
            Language::Vietnamese => f.write_str("Vietnamese"),
        }
    }
}
