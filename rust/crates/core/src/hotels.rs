//! The official Habbo hotels and the config.ini gamedata URLs that point at one of them.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HabboHotel {
    pub domain: &'static str,
    pub name: &'static str,
    pub region: &'static str,
    pub language: &'static str,
}

impl fmt::Display for HabboHotel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} - {} ({})", self.name, self.region, self.language)
    }
}

const fn hotel(domain: &'static str, name: &'static str, region: &'static str, language: &'static str) -> HabboHotel {
    HabboHotel { domain, name, region, language }
}

pub const ALL: &[HabboHotel] = &[
    hotel("habbo.com", "Habbo.com", "International", "English"),
    hotel("habbo.com.br", "Habbo.com.br", "Brazil", "Portuguese"),
    hotel("habbo.com.tr", "Habbo.com.tr", "Turkey", "Turkish"),
    hotel("habbo.de", "Habbo.de", "Germany", "German"),
    hotel("habbo.es", "Habbo.es", "Spain and Spanish-speaking regions", "Spanish"),
    hotel("habbo.fi", "Habbo.fi", "Finland", "Finnish"),
    hotel("habbo.fr", "Habbo.fr", "France", "French"),
    hotel("habbo.it", "Habbo.it", "Italy", "Italian"),
    hotel("habbo.nl", "Habbo.nl", "Netherlands and Belgium", "Dutch"),
];

/// The hotel-specific config.ini keys and their URL for this hotel.
pub fn urls(hotel: &HabboHotel) -> Vec<(&'static str, String)> {
    let d = hotel.domain;
    vec![
        ("externalvarsurl", format!("https://www.{d}/gamedata/external_variables/1")),
        ("externaltexturl", format!("https://{d}/gamedata/external_flash_texts/1")),
        ("productdataurl", format!("https://{d}/gamedata/productdata/1")),
        ("furnidataTXT", format!("https://{d}/gamedata/furnidata/1")),
        ("furnidataXML", format!("https://www.{d}/gamedata/furnidata_xml/1")),
    ]
}

/// The hotel all five URLs point at (with or without www.), or `None` for custom URLs.
pub fn detect(setting: impl Fn(&str) -> Option<String>) -> Option<HabboHotel> {
    ALL.iter().copied().find(|hotel| {
        urls(hotel)
            .iter()
            .all(|(key, url)| normalize(setting(key).as_deref()).eq_ignore_ascii_case(&normalize(Some(url))))
    })
}

fn normalize(url: Option<&str>) -> String {
    let url = url.unwrap_or("").trim().trim_end_matches('/');
    replace_ignore_case(&replace_ignore_case(url, "http://", "https://"), "://www.", "://")
}

fn replace_ignore_case(text: &str, from: &str, to: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (index, _) in lower.match_indices(&from.to_ascii_lowercase()) {
        out.push_str(&text[last..index]);
        out.push_str(to);
        last = index + from.len();
    }
    out.push_str(&text[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_nl_with_mixed_www_and_http() {
        let nl = ALL.iter().find(|h| h.domain == "habbo.nl").unwrap();
        let mut values = urls(nl);
        values[0].1 = "http://habbo.nl/gamedata/external_variables/1/".into();
        let found = detect(|key| values.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone()));
        assert_eq!(found.map(|h| h.domain), Some("habbo.nl"));
        assert_eq!(detect(|_| None), None);
    }
}
