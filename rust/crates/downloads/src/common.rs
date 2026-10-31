//! Shared plumbing for the downloaders: the HTTP calls with .NET's error split
//! (`HttpRequestException` vs. everything else), config.ini lookups, text decoding the way
//! `File.ReadAllLines` / `StreamReader` did it, and .NET-style number parsing.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use habbo_core::console::{self, Color};

/// `NullReferenceException.Message`.
pub const NULL_REFERENCE: &str = "Object reference not set to an instance of an object.";
/// `IndexOutOfRangeException.Message`.
pub const INDEX_OUT_OF_RANGE: &str = "Index was outside the bounds of the array.";

/// The error of one HTTP call, split the way the C# `catch` blocks split it.
#[derive(Debug)]
pub enum FetchError {
    /// `HttpRequestException`: a non-success status (`EnsureSuccessStatusCode`) or a network error.
    Http(String),
    /// Anything else (timeouts were `TaskCanceledException`, IO errors, bad URLs).
    Other(anyhow::Error),
}

impl FetchError {
    pub fn is_http(&self) -> bool {
        matches!(self, FetchError::Http(_))
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::Http(message) => f.write_str(message),
            FetchError::Other(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for FetchError {}

impl From<std::io::Error> for FetchError {
    fn from(error: std::io::Error) -> Self {
        FetchError::Other(error.into())
    }
}

impl From<anyhow::Error> for FetchError {
    fn from(error: anyhow::Error) -> Self {
        match error.downcast::<FetchError>() {
            Ok(fetch) => fetch,
            Err(other) => FetchError::Other(other),
        }
    }
}

/// The C# per-class `HttpClient` (100 s timeout, browser User-Agent).
pub fn default_client() -> &'static reqwest::Client {
    habbo_core::http::client()
}

/// A client with its own timeout (`HabboAssetDownloader.Http` used 2 minutes, the effect icons
/// download 15 minutes).
pub fn client_with_timeout(slot: &'static OnceLock<reqwest::Client>, timeout: Duration) -> &'static reqwest::Client {
    slot.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(habbo_core::http::USER_AGENT)
            .timeout(timeout)
            .pool_max_idle_per_host(64)
            .build()
            .expect("HTTP client")
    })
}

pub(crate) fn map_reqwest(error: reqwest::Error, timeout: Option<Duration>) -> FetchError {
    if error.is_timeout() {
        let seconds = timeout.map(|t| t.as_secs()).unwrap_or(100);
        return FetchError::Other(anyhow::anyhow!(
            "The request was canceled due to the configured HttpClient.Timeout of {seconds} seconds elapsing."
        ));
    }
    if error.is_builder() {
        return FetchError::Other(anyhow::anyhow!("An invalid request URI was provided. {error}"));
    }
    let mut message = error.to_string();
    let mut source = std::error::Error::source(&error);
    while let Some(inner) = source {
        message.push_str(": ");
        message.push_str(&inner.to_string());
        source = inner.source();
    }
    FetchError::Http(message)
}

/// .NET's `HttpRequestException` text for `EnsureSuccessStatusCode`.
pub fn status_message(status: reqwest::StatusCode) -> String {
    format!(
        "Response status code does not indicate success: {} ({}).",
        status.as_u16(),
        status.canonical_reason().unwrap_or("")
    )
}

/// `HttpStatusCode.ToString()`: the enum member name, or the number for an undefined code.
pub fn status_name(status: reqwest::StatusCode) -> String {
    let name = match status.as_u16() {
        100 => "Continue",
        101 => "SwitchingProtocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "NonAuthoritativeInformation",
        204 => "NoContent",
        205 => "ResetContent",
        206 => "PartialContent",
        300 => "Ambiguous",
        301 => "Moved",
        302 => "Found",
        303 => "RedirectMethod",
        304 => "NotModified",
        305 => "UseProxy",
        307 => "RedirectKeepVerb",
        308 => "PermanentRedirect",
        400 => "BadRequest",
        401 => "Unauthorized",
        402 => "PaymentRequired",
        403 => "Forbidden",
        404 => "NotFound",
        405 => "MethodNotAllowed",
        406 => "NotAcceptable",
        407 => "ProxyAuthenticationRequired",
        408 => "RequestTimeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "LengthRequired",
        412 => "PreconditionFailed",
        413 => "RequestEntityTooLarge",
        414 => "RequestUriTooLong",
        415 => "UnsupportedMediaType",
        416 => "RequestedRangeNotSatisfiable",
        417 => "ExpectationFailed",
        421 => "MisdirectedRequest",
        422 => "UnprocessableEntity",
        423 => "Locked",
        424 => "FailedDependency",
        426 => "UpgradeRequired",
        428 => "PreconditionRequired",
        429 => "TooManyRequests",
        431 => "RequestHeaderFieldsTooLarge",
        451 => "UnavailableForLegalReasons",
        500 => "InternalServerError",
        501 => "NotImplemented",
        502 => "BadGateway",
        503 => "ServiceUnavailable",
        504 => "GatewayTimeout",
        505 => "HttpVersionNotSupported",
        506 => "VariantAlsoNegotiates",
        507 => "InsufficientStorage",
        508 => "LoopDetected",
        510 => "NotExtended",
        511 => "NetworkAuthenticationRequired",
        other => return other.to_string(),
    };
    name.to_string()
}

/// `HttpClient.GetAsync`: the response whatever its status.
pub async fn send(client: &reqwest::Client, url: &str) -> Result<reqwest::Response, FetchError> {
    send_with(client.get(url), None).await
}

pub async fn send_with(request: reqwest::RequestBuilder, timeout: Option<Duration>) -> Result<reqwest::Response, FetchError> {
    request.send().await.map_err(|e| map_reqwest(e, timeout))
}

/// The body of a response.
pub async fn body(response: reqwest::Response) -> Result<Vec<u8>, FetchError> {
    response.bytes().await.map(|b| b.to_vec()).map_err(|e| map_reqwest(e, None))
}

/// `GetAsync` + `EnsureSuccessStatusCode` + read the body (`GetByteArrayAsync`).
pub async fn get_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, FetchError> {
    let response = send(client, url).await?;
    ensure_success(&response)?;
    body(response).await
}

/// `GetStringAsync`.
pub async fn get_string(client: &reqwest::Client, url: &str) -> Result<String, FetchError> {
    Ok(decode_text(&get_bytes(client, url).await?))
}

/// `EnsureSuccessStatusCode`.
pub fn ensure_success(response: &reqwest::Response) -> Result<(), FetchError> {
    if response.status().is_success() {
        Ok(())
    } else {
        Err(FetchError::Http(status_message(response.status())))
    }
}

/// Text from bytes the way .NET's `StreamReader` read it: the BOM picks UTF-8/UTF-16, else UTF-8.
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// `File.ReadAllLines` / `StreamReader.ReadLine` loop: lines split on "\r\n", "\r" or "\n".
pub fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                lines.push(&text[start..i]);
                start = i + 1;
            }
            b'\r' => {
                lines.push(&text[start..i]);
                if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    i += 1;
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < bytes.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// `File.ReadAllLines`.
pub fn read_lines(path: impl AsRef<Path>) -> std::io::Result<Vec<String>> {
    let text = decode_text(&std::fs::read(path)?);
    Ok(split_lines(&text).into_iter().map(str::to_string).collect())
}

/// `config["AppSettings:key"]` (throws `KeyNotFoundException` when the key is missing).
pub fn cfg(config: &HashMap<String, String>, key: &str) -> anyhow::Result<String> {
    config
        .get(key)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("The given key '{key}' was not present in the dictionary."))
}

/// The default of config.ini `download_parallelism`.
pub const DEFAULT_DOWNLOAD_PARALLELISM: usize = 16;
/// The largest config.ini `download_parallelism` taken.
pub const MAX_DOWNLOAD_PARALLELISM: usize = 64;

/// config.ini `download_parallelism` (not in the C# version): how many files the downloaders of
/// many small files fetch at once. Missing or not a number: 16; clamped to 1..=64.
pub fn parse_download_parallelism(value: Option<&str>) -> usize {
    match value.map(str::trim).and_then(|v| v.parse::<i64>().ok()) {
        Some(n) => n.clamp(1, MAX_DOWNLOAD_PARALLELISM as i64) as usize,
        None => DEFAULT_DOWNLOAD_PARALLELISM,
    }
}

/// [`parse_download_parallelism`] of the current config.ini.
pub fn download_parallelism() -> usize {
    parse_download_parallelism(habbo_core::settings::read_setting("download_parallelism").as_deref())
}

/// The key that tells two paths of this run apart the way the file system does (Windows
/// ignores case), for downloaders that must not fetch one file twice at once.
pub fn path_key(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

/// `IniFileParser.Parse("config.ini")`.
pub fn load_config() -> anyhow::Result<HashMap<String, String>> {
    habbo_core::ini::parse("config.ini")
}

/// `int.TryParse` / `XmlConvert.ToInt32` (NumberStyles.Integer, invariant culture).
pub fn parse_int(text: &str) -> Option<i32> {
    let trimmed = text.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{000B}' | '\u{000C}'));
    let (negative, digits) = match trimmed.as_bytes().first() {
        Some(b'-') => (true, &trimmed[1..]),
        Some(b'+') => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut value: i64 = 0;
    for b in digits.bytes() {
        value = value * 10 + i64::from(b - b'0');
        if value > i64::from(i32::MAX) + 1 {
            return None;
        }
    }
    let value = if negative { -value } else { value };
    i32::try_from(value).ok()
}

/// `int.Parse`: the .NET exception messages on failure.
pub fn parse_int_strict(text: Option<&str>) -> anyhow::Result<i32> {
    let Some(text) = text else {
        anyhow::bail!("Value cannot be null. (Parameter 's')");
    };
    if let Some(value) = parse_int(text) {
        return Ok(value);
    }
    let trimmed = text.trim();
    let digits = trimmed.trim_start_matches(['-', '+']);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        anyhow::bail!("Value was either too large or too small for an Int32.");
    }
    anyhow::bail!("The input string '{text}' was not in a correct format.")
}

/// `double.TryParse(s, NumberStyles.Float, CultureInfo.InvariantCulture)`.
pub fn parse_double(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed {
        "NaN" => return Some(f64::NAN),
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    // Rust also takes "inf"/"nan"; .NET does not.
    if trimmed.bytes().any(|b| b.is_ascii_alphabetic() && b != b'e' && b != b'E') {
        return None;
    }
    trimmed.parse::<f64>().ok()
}

static CONSOLE_LOCK: Mutex<()> = Mutex::new(());

/// One coloured line under a lock (the C# `lock (consoleLock)` blocks). Resets to Gray.
pub fn log(color: Color, message: &str) {
    let _guard = CONSOLE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    console::set_color(color);
    console::writeln(message);
    console::set_color(Color::Gray);
}

/// `Console.ForegroundColor = color; WriteLine(message); ForegroundColor = Gray`.
pub fn colored(color: Color, message: &str) {
    console::set_color(color);
    console::writeln(message);
    console::set_color(Color::Gray);
}

/// The release folder of a `flash.client.url=` line as `VersionChecker` / the clothes and
/// effects downloaders read it: `source.Split(Environment.NewLine.ToCharArray())`, the last
/// character dropped, then part 4 of a split on '/'.
/// `Ok(None)` when no line matches; `Err(parts)` when the line has 4 parts or fewer.
pub fn release_from_line_drop_last(source: &str) -> Option<Result<String, usize>> {
    for line in source.split(['\r', '\n']) {
        if line.contains("flash.client.url=") {
            let mut chars = line.chars();
            chars.next_back();
            let parts: Vec<&str> = chars.as_str().split('/').collect();
            return Some(if parts.len() > 4 { Ok(parts[4].to_string()) } else { Err(parts.len()) });
        }
    }
    None
}

/// `HabboAssetDownloader.GetReleaseAsync`'s parsing: trailing '/' trimmed, part 4.
pub fn release_trim_end(source: &str) -> Option<String> {
    for line in source.split(['\n', '\r']) {
        if !line.contains("flash.client.url=") {
            continue;
        }
        let parts: Vec<&str> = line.trim_end_matches('/').split('/').collect();
        if parts.len() > 4 {
            return Some(parts[4].to_string());
        }
    }
    None
}

/// `Directory.Delete("./temp")` after deleting its files, as the C# `finally` blocks did.
pub fn clean_temp() -> std::io::Result<()> {
    let temp = Path::new("./temp");
    if temp.is_dir() {
        for entry in std::fs::read_dir(temp)?.flatten() {
            if entry.path().is_file() {
                std::fs::remove_file(entry.path())?;
            }
        }
        std::fs::remove_dir(temp)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_split_like_streamreader() {
        assert_eq!(split_lines("a\r\nb\rc\n\nd"), ["a", "b", "c", "", "d"]);
        assert_eq!(split_lines("a\n"), ["a"]);
        assert!(split_lines("").is_empty());
        assert_eq!(decode_text(b"\xEF\xBB\xBFhi"), "hi");
    }

    #[test]
    fn ints_parse_like_dotnet() {
        assert_eq!(parse_int(" 12 "), Some(12));
        assert_eq!(parse_int("-5"), Some(-5));
        assert_eq!(parse_int("+7"), Some(7));
        assert_eq!(parse_int("1.0"), None);
        assert_eq!(parse_int("2147483648"), None);
        assert_eq!(parse_int("-2147483648"), Some(i32::MIN));
        assert!(parse_int_strict(Some("x")).unwrap_err().to_string().contains("'x' was not in a correct format"));
        assert_eq!(parse_double("-0.9"), Some(-0.9));
        assert_eq!(parse_double("inf"), None);
    }

    #[test]
    fn release_parsing() {
        let source = "a=b\r\nflash.client.url=https://images.habbo.com/gordon/flash-assets-PRODUCTION-1/\r\nx=y";
        assert_eq!(release_from_line_drop_last(source), Some(Ok("flash-assets-PRODUCTION-1".into())));
        assert_eq!(release_trim_end(source), Some("flash-assets-PRODUCTION-1".into()));
        assert_eq!(release_from_line_drop_last("flash.client.url=a/b/"), Some(Err(2)));
        assert_eq!(release_from_line_drop_last("nothing"), None);
        assert_eq!(release_trim_end("flash.client.url=a/b/"), None);
    }

    #[test]
    fn download_parallelism_setting() {
        assert_eq!(parse_download_parallelism(None), 16);
        assert_eq!(parse_download_parallelism(Some("")), 16);
        assert_eq!(parse_download_parallelism(Some("abc")), 16);
        assert_eq!(parse_download_parallelism(Some("4.5")), 16);
        assert_eq!(parse_download_parallelism(Some(" 24 ")), 24);
        assert_eq!(parse_download_parallelism(Some("1")), 1);
        assert_eq!(parse_download_parallelism(Some("0")), 1);
        assert_eq!(parse_download_parallelism(Some("-3")), 1);
        assert_eq!(parse_download_parallelism(Some("64")), 64);
        assert_eq!(parse_download_parallelism(Some("1000")), 64);
        assert_eq!(parse_download_parallelism(Some("99999999999999999999")), 16);
    }

    #[test]
    fn path_keys() {
        assert_eq!(path_key(Path::new("a/b.png")), path_key(Path::new("a\\b.png")));
        if cfg!(windows) {
            assert_eq!(path_key(Path::new("A/B.png")), path_key(Path::new("a/b.PNG")));
        }
    }

    #[test]
    fn status_names() {
        assert_eq!(status_name(reqwest::StatusCode::NOT_FOUND), "NotFound");
        assert_eq!(
            status_message(reqwest::StatusCode::NOT_FOUND),
            "Response status code does not indicate success: 404 (Not Found)."
        );
    }
}
