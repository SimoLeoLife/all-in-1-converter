//! "Test connection" for the GUI (new behaviour, not in the C# tool): connects with the values
//! typed in the form (not config.ini) and reports the server version, the database and the
//! items_base row count, or one readable line saying what went wrong.

use std::time::Duration;

use anyhow::{anyhow, Result};
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder};

/// How long the whole test (connect + queries) may take.
pub const TEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Connects with the given settings and returns e.g.
/// `Connected: MariaDB 12.3.3 · database arcturus · 63 items_base rows`
/// (the row count only when the table exists). The error is a single readable line.
pub async fn test_connection(server: &str, port: &str, user: &str, password: &str, database: &str) -> Result<String> {
    let server = server.trim();
    let database = database.trim();
    if server.is_empty() {
        return Err(anyhow!("No server given."));
    }
    let port_number: u16 = port.trim().parse().map_err(|_| anyhow!("'{}' is not a valid port (1-65535).", port.trim()))?;
    if port_number == 0 {
        return Err(anyhow!("'0' is not a valid port (1-65535)."));
    }
    let opts = OptsBuilder::default()
        .ip_or_hostname(server.to_string())
        .tcp_port(port_number)
        .user(Some(user.trim().to_string()))
        .pass(if password.is_empty() { None } else { Some(password.to_string()) })
        .db_name(if database.is_empty() { None } else { Some(database.to_string()) })
        .prefer_socket(false);

    let target = Target { server, port: port_number, user: user.trim(), database };
    match tokio::time::timeout(TEST_TIMEOUT, run(opts, database)).await {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(error)) => Err(anyhow!(describe_error(&error, &target))),
        Err(_) => Err(anyhow!(
            "No answer from {}:{} within {} s (host unreachable, wrong port or a firewall).",
            server,
            port_number,
            TEST_TIMEOUT.as_secs()
        )),
    }
}

async fn run(opts: OptsBuilder, database: &str) -> std::result::Result<String, mysql_async::Error> {
    let mut conn = Conn::new(opts).await?;
    let result = summary(&mut conn, database).await;
    let _ = conn.disconnect().await;
    result
}

async fn summary(conn: &mut Conn, database: &str) -> std::result::Result<String, mysql_async::Error> {
    let version: Option<String> = conn.query_first("SELECT VERSION()").await?;
    let mut text = format!("Connected: {}", server_name(version.as_deref().unwrap_or("")));
    if database.is_empty() {
        text.push_str(" · no database selected");
        return Ok(text);
    }
    text.push_str(&format!(" · database {database}"));
    let exists: Option<u64> = conn
        .query_first("SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name = 'items_base'")
        .await?;
    if exists.unwrap_or(0) > 0 {
        let rows: Option<u64> = conn.query_first("SELECT COUNT(*) FROM `items_base`").await?;
        text.push_str(&format!(" · {} items_base rows", habbo_core::console::n0(rows.unwrap_or(0))));
    } else {
        text.push_str(" · no items_base table");
    }
    Ok(text)
}

/// `12.3.3-MariaDB-log` -> `MariaDB 12.3.3`, `8.0.36` -> `MySQL 8.0.36`.
pub(crate) fn server_name(version: &str) -> String {
    let number: String = version.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let number = if number.is_empty() { version.to_string() } else { number };
    // MariaDB 10.x used to report "5.5.5-10.x.y-MariaDB" over the wire; VERSION() is the real one.
    if version.to_ascii_lowercase().contains("mariadb") {
        format!("MariaDB {number}")
    } else {
        format!("MySQL {number}")
    }
}

pub(crate) struct Target<'a> {
    pub server: &'a str,
    pub port: u16,
    pub user: &'a str,
    pub database: &'a str,
}

/// One readable line for a connection error.
pub(crate) fn describe_error(error: &mysql_async::Error, target: &Target) -> String {
    use mysql_async::Error;
    match error {
        Error::Server(server) => match server.code {
            1045 => format!("Access denied for user '{}': wrong user name or password.", target.user),
            1044 => format!("User '{}' has no access to database '{}'.", target.user, target.database),
            1049 => format!("Unknown database '{}' on {}:{}.", target.database, target.server, target.port),
            1130 => format!("Host not allowed: the server at {}:{} does not accept connections from this computer for user '{}'.", target.server, target.port, target.user),
            1040 => "The server has too many connections.".to_string(),
            _ => format!("Server error {}: {}", server.code, one_line(&server.message)),
        },
        Error::Io(io) => {
            let detail = one_line(&io.to_string());
            if let mysql_async::IoError::Io(e) = io {
                match e.kind() {
                    std::io::ErrorKind::ConnectionRefused => {
                        return format!("Cannot connect to {}:{}: connection refused (is the MySQL / MariaDB server running on that port?).", target.server, target.port)
                    }
                    std::io::ErrorKind::NotFound => return format!("Host not found: '{}'.", target.server),
                    _ => {}
                }
            }
            format!("Cannot reach {}:{}: {detail}", target.server, target.port)
        }
        // MariaDB on Windows offers root a second way in (auth_gssapi_client, Windows login)
        // after a wrong password; the driver cannot do it, so it is still a wrong password.
        Error::Driver(mysql_async::DriverError::UnknownAuthPlugin { name }) => format!(
            "Access denied for user '{}': wrong user name or password (the server then asked for the unsupported '{name}' login).",
            target.user
        ),
        other => one_line(&other.to_string()),
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_names() {
        assert_eq!(server_name("12.3.3-MariaDB"), "MariaDB 12.3.3");
        assert_eq!(server_name("10.11.6-MariaDB-0+deb12u1-log"), "MariaDB 10.11.6");
        assert_eq!(server_name("8.0.36"), "MySQL 8.0.36");
        assert_eq!(server_name("8.4.0-commercial"), "MySQL 8.4.0");
    }

    #[test]
    fn error_lines() {
        let target = Target { server: "db", port: 3306, user: "root", database: "hotel" };
        let server = |code| mysql_async::Error::Server(mysql_async::ServerError { code, message: "x\ny".into(), state: "28000".into() });
        assert_eq!(describe_error(&server(1045), &target), "Access denied for user 'root': wrong user name or password.");
        assert_eq!(describe_error(&server(1049), &target), "Unknown database 'hotel' on db:3306.");
        assert_eq!(describe_error(&server(9999), &target), "Server error 9999: x y");
        let refused = mysql_async::Error::Io(mysql_async::IoError::Io(std::io::Error::from(std::io::ErrorKind::ConnectionRefused)));
        assert!(describe_error(&refused, &target).starts_with("Cannot connect to db:3306: connection refused"));
    }

    #[tokio::test]
    async fn bad_port_is_one_line() {
        let error = test_connection("127.0.0.1", "abc", "root", "", "x").await.unwrap_err();
        assert_eq!(error.to_string(), "'abc' is not a valid port (1-65535).");
    }
}
