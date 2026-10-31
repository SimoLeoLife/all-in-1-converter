//! `DatabaseConfig`: the MySQL / MariaDB connection settings from config.ini, loaded once.

use std::sync::OnceLock;

use habbo_core::outln;
use mysql_async::{Opts, OptsBuilder};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseConfig {
    pub database_server: String,
    pub port: String,
    pub username: String,
    pub password: String,
    pub database_name: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            database_server: "127.0.0.1".into(),
            port: "3306".into(),
            username: "root".into(),
            password: String::new(),
            database_name: "HABBO".into(),
        }
    }
}

const CONFIG_FILE_PATH: &str = "config.ini";

impl DatabaseConfig {
    /// Applies every `DATABASE*=` line (case-insensitive prefix, anywhere in the file, the last one wins).
    pub fn from_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> Self {
        let mut config = Self::default();
        for line in lines {
            if let Some(value) = strip_prefix_ignore_case(line, "DATABASESERVER=") {
                config.database_server = value.trim().to_string();
            } else if let Some(value) = strip_prefix_ignore_case(line, "DATABASEPORT=") {
                config.port = value.trim().to_string();
            } else if let Some(value) = strip_prefix_ignore_case(line, "DATABASEUSER=") {
                config.username = value.trim().to_string();
            } else if let Some(value) = strip_prefix_ignore_case(line, "DATABASEPASSWORD=") {
                config.password = value.trim().to_string();
            } else if let Some(value) = strip_prefix_ignore_case(line, "DATABASENAME=") {
                config.database_name = value.trim().to_string();
            }
        }
        config
    }

    fn load() -> Self {
        if !std::path::Path::new(CONFIG_FILE_PATH).is_file() {
            outln!("config.ini file not found, using default connection settings.");
            return Self::default();
        }
        // NOTE(port): an unreadable config.ini made the C# static constructor throw; here it
        // falls back to the defaults.
        let bytes = std::fs::read(CONFIG_FILE_PATH).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        // File.ReadAllLines splits on \r\n, \n and \r. The shipped config.ini comes first, so a
        // file without some DATABASE* line still gets its value.
        let defaults = habbo_core::ini::DEFAULT_CONFIG.lines();
        Self::from_lines(defaults.chain(text.split("\r\n").flat_map(|l| l.split(['\n', '\r']))))
    }

    /// The C# `ConnectionString` text.
    pub fn connection_string(&self) -> String {
        format!(
            "server={};port={};user={};password={};database={};",
            self.database_server, self.port, self.username, self.password, self.database_name
        )
    }

    /// The same settings as mysql_async options.
    pub fn opts(&self) -> anyhow::Result<Opts> {
        let port: u16 = self
            .port
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("The value '{}' is not a valid port.", self.port))?;
        let builder = OptsBuilder::default()
            .ip_or_hostname(self.database_server.clone())
            .tcp_port(port)
            .user(Some(self.username.clone()))
            .pass(if self.password.is_empty() { None } else { Some(self.password.clone()) })
            .db_name(if self.database_name.is_empty() { None } else { Some(self.database_name.clone()) });
        Ok(builder.into())
    }
}

fn strip_prefix_ignore_case<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    if line.len() >= prefix.len() && line.is_char_boundary(prefix.len()) && line[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&line[prefix.len()..])
    } else {
        None
    }
}

static CONFIG: OnceLock<DatabaseConfig> = OnceLock::new();

/// The settings, read from config.ini the first time they are needed (the C# static constructor).
pub fn get() -> &'static DatabaseConfig {
    CONFIG.get_or_init(DatabaseConfig::load)
}

/// Opens a connection with the configured settings (`new MySqlConnection(..).OpenAsync()`).
pub async fn connect() -> anyhow::Result<mysql_async::Conn> {
    Ok(mysql_async::Conn::new(get().opts()?).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_database_lines() {
        let config = DatabaseConfig::from_lines(
            "[AppSettings]\ndatabaseserver= db.local \nDATABASEPORT=3307\nDatabaseUser=habbo\nDATABASEPASSWORD=p=w\n DATABASENAME=ignored\nDATABASENAME=hotel"
                .lines(),
        );
        assert_eq!(config.database_server, "db.local");
        assert_eq!(config.port, "3307");
        assert_eq!(config.username, "habbo");
        assert_eq!(config.password, "p=w");
        assert_eq!(config.database_name, "hotel");
        assert_eq!(config.connection_string(), "server=db.local;port=3307;user=habbo;password=p=w;database=hotel;");
        assert!(config.opts().is_ok());
    }

    #[test]
    fn defaults() {
        let config = DatabaseConfig::from_lines(std::iter::empty());
        assert_eq!(config.connection_string(), "server=127.0.0.1;port=3306;user=root;password=;database=HABBO;");
    }
}
