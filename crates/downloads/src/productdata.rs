//! `ProductDataDownloader` (Productdata.cs): productdata.txt + ProductData.json.

use std::sync::OnceLock;

use anyhow::{bail, Result};
use habbo_core::Color;
use regex::Regex;
use serde_json::{json, Value};

use crate::common::{self, colored, FetchError};
use crate::resume;
use crate::jsonw::{to_string_indented, Escaping};

const TEXT_FILE_PATH: &str = "./Habbo_Default/files/txt/productdata.txt";
const JSON_FILE_PATH: &str = "./Habbo_Default/files/json/ProductData.json";

/// `ProductDataDownloader.DownloadProductDataAsync`.
pub async fn download_product_data() -> Result<()> {
    let config = common::load_config()?;

    let result: Result<()> = async {
        let url = common::cfg(&config, "AppSettings:productdataurl")?;
        if url.is_empty() {
            colored(Color::Red, "❌ Error: Productdata URL is not configured.");
            return Ok(());
        }

        download_file(&url, TEXT_FILE_PATH, "productdata.txt").await?;
        colored(Color::Green, "✅ Productdata Saved");
        convert_product_data_to_json(TEXT_FILE_PATH);
        Ok(())
    }
    .await;
    if let Err(error) = result {
        colored(Color::Red, &format!("❌ Error downloading productdata: {error}"));
    }
    Ok(())
}

async fn download_file(url: &str, file_path: &str, file_name: &str) -> Result<(), FetchError> {
    let result: Result<(), FetchError> = resume::download_to(common::default_client(), url, std::path::Path::new(file_path), &resume::Options { create_dir: true, ..resume::Options::FRESH }).await;
    match &result {
        Ok(()) => colored(Color::Green, &format!("✅ Downloaded: {file_name}")),
        Err(FetchError::Http(message)) => colored(Color::Red, &format!("❌ Error downloading {file_name}: {message}")),
        Err(FetchError::Other(_)) => {}
    }
    result
}

fn convert_product_data_to_json(text_file_path: &str) {
    if !std::path::Path::new(text_file_path).is_file() {
        colored(Color::Red, "❌ Error: productdata.txt not found.");
        return;
    }
    let result = (|| -> Result<()> {
        let text = common::decode_text(&std::fs::read(text_file_path)?);
        let json = product_data_json(&text)?;
        if let Some(directory) = std::path::Path::new(JSON_FILE_PATH).parent() {
            std::fs::create_dir_all(directory)?;
        }
        std::fs::write(JSON_FILE_PATH, json)?;
        Ok(())
    })();
    match result {
        Ok(()) => colored(Color::Green, &format!("🔄 Converted: {JSON_FILE_PATH}")),
        Err(error) => colored(Color::Red, &format!("❌ Error converting productdata to JSON: {error}")),
    }
}

fn product_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\[\s*"(.*?)"\s*,\s*"(.*?)"\s*,\s*"(.*?)"\s*\]"#).unwrap())
}

/// `{ productdata: { product: [{ code, name, description }] } }` from productdata.txt.
pub fn product_data(text: &str) -> Result<Value> {
    let mut products = Vec::new();
    for captures in product_regex().captures_iter(text) {
        let code = captures[1].trim().to_string();
        let name = decode_unicode(captures[2].trim())?;
        let description = decode_unicode(captures[3].trim())?;
        products.push(json!({ "code": code, "name": name, "description": description }));
    }
    Ok(json!({ "productdata": { "product": products } }))
}

pub fn product_data_json(text: &str) -> Result<String> {
    Ok(to_string_indented(&product_data(text)?, Escaping::Relaxed))
}

/// `Regex.Unescape(HttpUtility.HtmlDecode(input))`.
pub fn decode_unicode(input: &str) -> Result<String> {
    regex_unescape(&html_decode(input))
}

/// `HttpUtility.HtmlDecode`: named (HTML 4) and numeric character references; unknown ones stay.
pub fn html_decode(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        // .NET looks for ';' and only decodes when no other '&' comes first.
        let semicolon = after.find(';');
        let next_amp = after.find('&');
        let decoded = match semicolon {
            Some(end) if next_amp.is_none_or(|a| a > end) => {
                let entity = &after[..end];
                decode_entity(entity).map(|text| (text, end + 1))
            }
            _ => None,
        };
        match decoded {
            Some((text, consumed)) => {
                out.push_str(&text);
                rest = &after[consumed..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn decode_entity(entity: &str) -> Option<String> {
    if let Some(number) = entity.strip_prefix('#') {
        let value = if let Some(hex) = number.strip_prefix(['x', 'X']) {
            if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            u32::from_str_radix(hex, 16).ok()?
        } else {
            if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            number.parse::<u32>().ok()?
        };
        // .NET writes a supplementary code point as a surrogate pair; lone surrogates cannot be a Rust char.
        return char::from_u32(value).map(String::from).or_else(|| (0xD800..0xE000).contains(&value).then(|| "\u{FFFD}".into()));
    }
    ENTITIES.iter().find(|(name, _)| *name == entity).map(|(_, cp)| char::from_u32(*cp).unwrap().to_string())
}

/// `Regex.Unescape`: `\n`, `\t`, `\uXXXX`, `\xXX`, octal, `\cX`; any other escaped symbol is itself.
pub fn regex_unescape(input: &str) -> Result<String> {
    if !input.contains('\\') {
        return Ok(input.to_string());
    }
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(&e) = chars.get(i) else {
            bail!("Invalid pattern '{input}' at offset {}. Illegal \\ at end of pattern.", chars.len());
        };
        i += 1;
        match e {
            '0'..='7' => {
                let mut value: u32 = e.to_digit(8).unwrap();
                let mut digits = 1;
                while digits < 3 && i < chars.len() && ('0'..='7').contains(&chars[i]) {
                    value = value * 8 + chars[i].to_digit(8).unwrap();
                    i += 1;
                    digits += 1;
                }
                out.push(char::from_u32(value & 0xFF).unwrap());
            }
            'x' | 'u' => {
                let count = if e == 'x' { 2 } else { 4 };
                if i + count > chars.len() {
                    bail!("Invalid pattern '{input}' at offset {}. Insufficient hexadecimal digits.", i);
                }
                let hex: String = chars[i..i + count].iter().collect();
                let Ok(value) = u32::from_str_radix(&hex, 16) else {
                    bail!("Invalid pattern '{input}' at offset {}. Insufficient hexadecimal digits.", i);
                };
                if hex.starts_with(['+', '-']) {
                    bail!("Invalid pattern '{input}' at offset {}. Insufficient hexadecimal digits.", i);
                }
                i += count;
                // A lone surrogate half (half of an escaped emoji) is joined with the next \uXXXX.
                if (0xD800..0xDC00).contains(&value)
                    && i + 6 <= chars.len()
                    && chars[i] == '\\'
                    && chars[i + 1] == 'u'
                {
                    let low: String = chars[i + 2..i + 6].iter().collect();
                    if let Ok(low) = u32::from_str_radix(&low, 16) {
                        if (0xDC00..0xE000).contains(&low) {
                            out.push(char::from_u32(0x10000 + ((value - 0xD800) << 10) + (low - 0xDC00)).unwrap());
                            i += 6;
                            continue;
                        }
                    }
                }
                // NOTE(port): a lone surrogate cannot live in a Rust string; it becomes U+FFFD.
                out.push(char::from_u32(value).unwrap_or('\u{FFFD}'));
            }
            'a' => out.push('\u{7}'),
            'b' => out.push('\u{8}'),
            'e' => out.push('\u{1B}'),
            'f' => out.push('\u{C}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'v' => out.push('\u{B}'),
            'c' => {
                let Some(&control) = chars.get(i) else {
                    bail!("Invalid pattern '{input}' at offset {}. Missing control character.", i);
                };
                i += 1;
                let upper = control.to_ascii_uppercase() as u32;
                if !(0x40..0x60).contains(&upper) {
                    bail!("Invalid pattern '{input}' at offset {}. Unrecognized control character.", i);
                }
                out.push(char::from_u32(upper - 0x40).unwrap());
            }
            other if other.is_alphanumeric() || other == '_' => {
                bail!("Invalid pattern '{input}' at offset {}. Unrecognized escape sequence \\{other}.", i);
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

/// The HTML 4 named character references `HttpUtility.HtmlDecode` knows (plus `apos`).
const ENTITIES: &[(&str, u32)] = &[
    ("quot", 34), ("amp", 38), ("apos", 39), ("lt", 60), ("gt", 62),
    ("nbsp", 160), ("iexcl", 161), ("cent", 162), ("pound", 163), ("curren", 164), ("yen", 165),
    ("brvbar", 166), ("sect", 167), ("uml", 168), ("copy", 169), ("ordf", 170), ("laquo", 171),
    ("not", 172), ("shy", 173), ("reg", 174), ("macr", 175), ("deg", 176), ("plusmn", 177),
    ("sup2", 178), ("sup3", 179), ("acute", 180), ("micro", 181), ("para", 182), ("middot", 183),
    ("cedil", 184), ("sup1", 185), ("ordm", 186), ("raquo", 187), ("frac14", 188), ("frac12", 189),
    ("frac34", 190), ("iquest", 191), ("Agrave", 192), ("Aacute", 193), ("Acirc", 194), ("Atilde", 195),
    ("Auml", 196), ("Aring", 197), ("AElig", 198), ("Ccedil", 199), ("Egrave", 200), ("Eacute", 201),
    ("Ecirc", 202), ("Euml", 203), ("Igrave", 204), ("Iacute", 205), ("Icirc", 206), ("Iuml", 207),
    ("ETH", 208), ("Ntilde", 209), ("Ograve", 210), ("Oacute", 211), ("Ocirc", 212), ("Otilde", 213),
    ("Ouml", 214), ("times", 215), ("Oslash", 216), ("Ugrave", 217), ("Uacute", 218), ("Ucirc", 219),
    ("Uuml", 220), ("Yacute", 221), ("THORN", 222), ("szlig", 223), ("agrave", 224), ("aacute", 225),
    ("acirc", 226), ("atilde", 227), ("auml", 228), ("aring", 229), ("aelig", 230), ("ccedil", 231),
    ("egrave", 232), ("eacute", 233), ("ecirc", 234), ("euml", 235), ("igrave", 236), ("iacute", 237),
    ("icirc", 238), ("iuml", 239), ("eth", 240), ("ntilde", 241), ("ograve", 242), ("oacute", 243),
    ("ocirc", 244), ("otilde", 245), ("ouml", 246), ("divide", 247), ("oslash", 248), ("ugrave", 249),
    ("uacute", 250), ("ucirc", 251), ("uuml", 252), ("yacute", 253), ("thorn", 254), ("yuml", 255),
    ("OElig", 338), ("oelig", 339), ("Scaron", 352), ("scaron", 353), ("Yuml", 376), ("fnof", 402),
    ("circ", 710), ("tilde", 732),
    ("Alpha", 913), ("Beta", 914), ("Gamma", 915), ("Delta", 916), ("Epsilon", 917), ("Zeta", 918),
    ("Eta", 919), ("Theta", 920), ("Iota", 921), ("Kappa", 922), ("Lambda", 923), ("Mu", 924),
    ("Nu", 925), ("Xi", 926), ("Omicron", 927), ("Pi", 928), ("Rho", 929), ("Sigma", 931),
    ("Tau", 932), ("Upsilon", 933), ("Phi", 934), ("Chi", 935), ("Psi", 936), ("Omega", 937),
    ("alpha", 945), ("beta", 946), ("gamma", 947), ("delta", 948), ("epsilon", 949), ("zeta", 950),
    ("eta", 951), ("theta", 952), ("iota", 953), ("kappa", 954), ("lambda", 955), ("mu", 956),
    ("nu", 957), ("xi", 958), ("omicron", 959), ("pi", 960), ("rho", 961), ("sigmaf", 962),
    ("sigma", 963), ("tau", 964), ("upsilon", 965), ("phi", 966), ("chi", 967), ("psi", 968),
    ("omega", 969), ("thetasym", 977), ("upsih", 978), ("piv", 982),
    ("ensp", 8194), ("emsp", 8195), ("thinsp", 8201), ("zwnj", 8204), ("zwj", 8205), ("lrm", 8206),
    ("rlm", 8207), ("ndash", 8211), ("mdash", 8212), ("lsquo", 8216), ("rsquo", 8217), ("sbquo", 8218),
    ("ldquo", 8220), ("rdquo", 8221), ("bdquo", 8222), ("dagger", 8224), ("Dagger", 8225), ("bull", 8226),
    ("hellip", 8230), ("permil", 8240), ("prime", 8242), ("Prime", 8243), ("lsaquo", 8249), ("rsaquo", 8250),
    ("oline", 8254), ("frasl", 8260), ("euro", 8364), ("image", 8465), ("weierp", 8472), ("real", 8476),
    ("trade", 8482), ("alefsym", 8501), ("larr", 8592), ("uarr", 8593), ("rarr", 8594), ("darr", 8595),
    ("harr", 8596), ("crarr", 8629), ("lArr", 8656), ("uArr", 8657), ("rArr", 8658), ("dArr", 8659),
    ("hArr", 8660), ("forall", 8704), ("part", 8706), ("exist", 8707), ("empty", 8709), ("nabla", 8711),
    ("isin", 8712), ("notin", 8713), ("ni", 8715), ("prod", 8719), ("sum", 8721), ("minus", 8722),
    ("lowast", 8727), ("radic", 8730), ("prop", 8733), ("infin", 8734), ("ang", 8736), ("and", 8743),
    ("or", 8744), ("cap", 8745), ("cup", 8746), ("int", 8747), ("there4", 8756), ("sim", 8764),
    ("cong", 8773), ("asymp", 8776), ("ne", 8800), ("equiv", 8801), ("le", 8804), ("ge", 8805),
    ("sub", 8834), ("sup", 8835), ("nsub", 8836), ("sube", 8838), ("supe", 8839), ("oplus", 8853),
    ("otimes", 8855), ("perp", 8869), ("sdot", 8901), ("lceil", 8968), ("rceil", 8969), ("lfloor", 8970),
    ("rfloor", 8971), ("lang", 9001), ("rang", 9002), ("loz", 9674), ("spades", 9824), ("clubs", 9827),
    ("hearts", 9829), ("diams", 9830),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn products_from_text() {
        let text = r#"[["shelves_norja","Beige Bookcase","For nic naks &amp; art"],[ "  a1 " , "Café" , "x\ny" ]]
[["broken","only two"]]"#;
        let value = product_data(text).unwrap();
        let products = value["productdata"]["product"].as_array().unwrap();
        assert_eq!(products.len(), 2);
        assert_eq!(products[0].to_string(), r#"{"code":"shelves_norja","name":"Beige Bookcase","description":"For nic naks & art"}"#);
        assert_eq!(products[1]["code"], "a1");
        assert_eq!(products[1]["name"], "Café");
        assert_eq!(products[1]["description"], "x\ny");
        let json = crate::jsonw::to_string_with_newline(&value, Escaping::Relaxed, "\n");
        assert!(json.starts_with("{\n  \"productdata\": {\n    \"product\": [\n      {\n        \"code\": \"shelves_norja\","));
    }

    #[test]
    fn html_and_regex_unescape() {
        assert_eq!(html_decode("a &lt;b&gt; &#233; &#x41; &euro; &bogus; & x"), "a <b> é A € &bogus; & x");
        assert_eq!(html_decode("&amp&lt;"), "&amp<");
        assert_eq!(regex_unescape(r"a\tb\\cé\x41\101\.😀").unwrap(), "a\tb\\cé\u{41}A.😀");
        assert!(regex_unescape(r"bad\q").is_err());
        assert!(regex_unescape("end\\").is_err());
        assert!(regex_unescape(r"\u12").is_err());
        assert_eq!(regex_unescape(r"\cA").unwrap(), "\u{1}");
        assert!(product_data(r#"["a","b\q","c"]"#).is_err());
    }
}
