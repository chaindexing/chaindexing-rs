use std::borrow::Cow;
use std::fmt;
use std::ops::Range;
use std::path::PathBuf;

use diesel::{ConnectionError, ConnectionResult};
use diesel_async::AsyncPgConnection;
use tokio_postgres::{config::SslMode, Client, Config as TokioPostgresConfig, NoTls, Socket};

/// Postgres TLS policy used by Chaindexing's Diesel pool and raw clients.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PostgresTlsMode {
    /// Never use TLS.
    Disable,
    /// Try TLS and fall back to cleartext only when the server does not offer TLS.
    Prefer,
    /// Require TLS.
    Require,
}

/// TLS configuration for Postgres connections.
#[derive(Clone, Eq, PartialEq)]
pub struct PostgresTlsConfig {
    mode: PostgresTlsMode,
    native_roots: bool,
    ca_certs_pem: Vec<Vec<u8>>,
    ca_cert_paths: Vec<PathBuf>,
}

impl fmt::Debug for PostgresTlsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresTlsConfig")
            .field("mode", &self.mode)
            .field("native_roots", &self.native_roots)
            .field("ca_certs_pem_count", &self.ca_certs_pem.len())
            .field("ca_cert_path_count", &self.ca_cert_paths.len())
            .finish()
    }
}

impl Default for PostgresTlsConfig {
    fn default() -> Self {
        Self::disable()
    }
}

impl PostgresTlsConfig {
    /// Creates a config that never uses TLS.
    pub fn disable() -> Self {
        Self {
            mode: PostgresTlsMode::Disable,
            native_roots: false,
            ca_certs_pem: vec![],
            ca_cert_paths: vec![],
        }
    }

    /// Creates a config that tries TLS and accepts cleartext if the server does not offer TLS.
    pub fn prefer() -> Self {
        Self::with_mode(PostgresTlsMode::Prefer)
    }

    /// Creates a config that requires TLS.
    pub fn require() -> Self {
        Self::with_mode(PostgresTlsMode::Require)
    }

    /// Creates a config from a TLS mode.
    pub fn with_mode(mode: PostgresTlsMode) -> Self {
        Self {
            mode,
            native_roots: mode != PostgresTlsMode::Disable,
            ca_certs_pem: vec![],
            ca_cert_paths: vec![],
        }
    }

    /// Infers TLS behavior from a Postgres URL or libpq-style connection string.
    ///
    /// A missing `sslmode` keeps the historical Chaindexing default: TLS is disabled.
    /// `sslmode=require`, `sslmode=verify-ca`, and `sslmode=verify-full` require TLS.
    pub fn from_database_url(database_url: &str) -> Self {
        let mut config = match sslmode_value(database_url).as_deref() {
            Some("prefer") | Some("allow") => Self::prefer(),
            Some("require") | Some("verify-ca") | Some("verify-full") => Self::require(),
            _ => Self::disable(),
        };

        if let Some(path) = sslrootcert_value(database_url) {
            config = config.with_ca_cert_path(path);
        }

        config
    }

    /// Returns the configured TLS mode.
    pub fn mode(&self) -> PostgresTlsMode {
        self.mode
    }

    /// Enables or disables loading the platform-native certificate roots.
    pub fn with_native_roots(mut self, enabled: bool) -> Self {
        self.native_roots = enabled;
        self
    }

    /// Disables platform-native roots. Use this when the supplied CA bundle should be exclusive.
    pub fn without_native_roots(self) -> Self {
        self.with_native_roots(false)
    }

    /// Adds a PEM-encoded CA certificate bundle trusted for server certificate verification.
    pub fn with_ca_cert_pem(mut self, pem: impl Into<Vec<u8>>) -> Self {
        self.ca_certs_pem.push(pem.into());
        self
    }

    /// Adds a PEM-encoded CA certificate bundle path trusted for server certificate verification.
    pub fn with_ca_cert_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.ca_cert_paths.push(path.into());
        self
    }
}

pub(crate) async fn connect_raw_client(
    database_url: &str,
    tls_config: &PostgresTlsConfig,
) -> Result<Client, String> {
    match tls_config.mode {
        PostgresTlsMode::Disable => {
            connect_raw_client_no_tls(database_url, PostgresTlsMode::Disable).await
        }
        PostgresTlsMode::Prefer => {
            connect_raw_client_with_preferred_tls(database_url, tls_config).await
        }
        PostgresTlsMode::Require => connect_raw_client_with_tls(database_url, tls_config).await,
    }
}

pub(crate) async fn connect_diesel(
    database_url: &str,
    tls_config: &PostgresTlsConfig,
) -> ConnectionResult<AsyncPgConnection> {
    match tls_config.mode {
        PostgresTlsMode::Disable => {
            connect_diesel_no_tls(database_url, PostgresTlsMode::Disable).await
        }
        PostgresTlsMode::Prefer => {
            connect_diesel_with_preferred_tls(database_url, tls_config).await
        }
        PostgresTlsMode::Require => connect_diesel_with_tls(database_url, tls_config).await,
    }
}

async fn connect_raw_client_no_tls(
    database_url: &str,
    mode: PostgresTlsMode,
) -> Result<Client, String> {
    let config = postgres_config(database_url, mode)?;
    let (client, conn) = config.connect(NoTls).await.map_err(connection_error)?;

    spawn_connection(conn);

    Ok(client)
}

async fn connect_diesel_no_tls(
    database_url: &str,
    mode: PostgresTlsMode,
) -> ConnectionResult<AsyncPgConnection> {
    let config = postgres_config(database_url, mode).map_err(bad_connection)?;
    let (client, conn) = config.connect(NoTls).await.map_err(bad_connection)?;

    AsyncPgConnection::try_from_client_and_connection(client, conn).await
}

#[cfg(feature = "postgres-rustls")]
async fn connect_raw_client_with_preferred_tls(
    database_url: &str,
    tls_config: &PostgresTlsConfig,
) -> Result<Client, String> {
    let tls = rustls_connector(tls_config)?;
    let config = postgres_config(database_url, PostgresTlsMode::Prefer)?;
    let (client, conn) = config.connect(tls).await.map_err(connection_error)?;

    spawn_connection(conn);

    Ok(client)
}

#[cfg(not(feature = "postgres-rustls"))]
async fn connect_raw_client_with_preferred_tls(
    database_url: &str,
    _tls_config: &PostgresTlsConfig,
) -> Result<Client, String> {
    connect_raw_client_no_tls(database_url, PostgresTlsMode::Prefer).await
}

#[cfg(feature = "postgres-rustls")]
async fn connect_diesel_with_preferred_tls(
    database_url: &str,
    tls_config: &PostgresTlsConfig,
) -> ConnectionResult<AsyncPgConnection> {
    let tls = rustls_connector(tls_config).map_err(bad_connection)?;
    let config = postgres_config(database_url, PostgresTlsMode::Prefer).map_err(bad_connection)?;
    let (client, conn) = config.connect(tls).await.map_err(bad_connection)?;

    AsyncPgConnection::try_from_client_and_connection(client, conn).await
}

#[cfg(not(feature = "postgres-rustls"))]
async fn connect_diesel_with_preferred_tls(
    database_url: &str,
    _tls_config: &PostgresTlsConfig,
) -> ConnectionResult<AsyncPgConnection> {
    connect_diesel_no_tls(database_url, PostgresTlsMode::Prefer).await
}

#[cfg(feature = "postgres-rustls")]
async fn connect_raw_client_with_tls(
    database_url: &str,
    tls_config: &PostgresTlsConfig,
) -> Result<Client, String> {
    let tls = rustls_connector(tls_config)?;
    let config = postgres_config(database_url, PostgresTlsMode::Require)?;
    let (client, conn) = config.connect(tls).await.map_err(connection_error)?;

    spawn_connection(conn);

    Ok(client)
}

#[cfg(not(feature = "postgres-rustls"))]
async fn connect_raw_client_with_tls(
    _database_url: &str,
    _tls_config: &PostgresTlsConfig,
) -> Result<Client, String> {
    Err("Postgres TLS requires the `postgres-rustls` feature".to_string())
}

#[cfg(feature = "postgres-rustls")]
async fn connect_diesel_with_tls(
    database_url: &str,
    tls_config: &PostgresTlsConfig,
) -> ConnectionResult<AsyncPgConnection> {
    let tls = rustls_connector(tls_config).map_err(bad_connection)?;
    let config = postgres_config(database_url, PostgresTlsMode::Require).map_err(bad_connection)?;
    let (client, conn) = config.connect(tls).await.map_err(bad_connection)?;

    AsyncPgConnection::try_from_client_and_connection(client, conn).await
}

#[cfg(not(feature = "postgres-rustls"))]
async fn connect_diesel_with_tls(
    _database_url: &str,
    _tls_config: &PostgresTlsConfig,
) -> ConnectionResult<AsyncPgConnection> {
    Err(ConnectionError::BadConnection(
        "Postgres TLS requires the `postgres-rustls` feature".to_string(),
    ))
}

fn postgres_config(
    database_url: &str,
    mode: PostgresTlsMode,
) -> Result<TokioPostgresConfig, String> {
    let database_url = normalize_connection_string_for_tokio_postgres(database_url);
    let mut config = database_url.parse::<TokioPostgresConfig>().map_err(connection_error)?;

    config.ssl_mode(tokio_ssl_mode(mode));

    Ok(config)
}

fn tokio_ssl_mode(mode: PostgresTlsMode) -> SslMode {
    match mode {
        PostgresTlsMode::Disable => SslMode::Disable,
        PostgresTlsMode::Prefer => SslMode::Prefer,
        PostgresTlsMode::Require => SslMode::Require,
    }
}

fn spawn_connection<S>(conn: tokio_postgres::Connection<Socket, S>)
where
    S: tokio_postgres::tls::TlsStream + Unpin + Send + 'static,
{
    tokio::spawn(async move { conn.await.map_err(|e| eprintln!("connection error: {e}")) });
}

fn bad_connection(error: impl ToString) -> ConnectionError {
    ConnectionError::BadConnection(error.to_string())
}

fn connection_error(error: impl ToString) -> String {
    error.to_string()
}

#[cfg(feature = "postgres-rustls")]
fn rustls_connector(
    tls_config: &PostgresTlsConfig,
) -> Result<tokio_postgres_rustls::MakeRustlsConnect, String> {
    let mut roots = rustls::RootCertStore::empty();

    if tls_config.native_roots {
        let native_certs = rustls_native_certs::load_native_certs();
        if native_certs.certs.is_empty() && !native_certs.errors.is_empty() {
            return Err(format!(
                "could not load native root certificates: {:?}",
                native_certs.errors
            ));
        }
        roots.add_parsable_certificates(native_certs.certs);
    }

    for pem in &tls_config.ca_certs_pem {
        add_pem_certs(&mut roots, pem.as_slice(), "Postgres CA certificate PEM")?;
    }

    for path in &tls_config.ca_cert_paths {
        let pem = std::fs::read(path).map_err(|e| {
            format!(
                "could not read Postgres CA certificate file `{}`: {e}",
                path.display()
            )
        })?;
        add_pem_certs(&mut roots, pem.as_slice(), &format!("`{}`", path.display()))?;
    }

    if roots.roots.is_empty() {
        return Err("Postgres TLS requires at least one trusted root certificate".to_string());
    }

    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    Ok(tokio_postgres_rustls::MakeRustlsConnect::new(config))
}

#[cfg(feature = "postgres-rustls")]
fn add_pem_certs(
    roots: &mut rustls::RootCertStore,
    pem: &[u8],
    description: &str,
) -> Result<(), String> {
    use std::io::Cursor;

    let certs = rustls_pemfile::certs(&mut Cursor::new(pem))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("could not parse {description}: {e}"))?;

    if certs.is_empty() {
        return Err(format!("{description} did not contain any certificates"));
    }

    let (added, _ignored) = roots.add_parsable_certificates(certs);
    if added == 0 {
        return Err(format!(
            "{description} did not contain any valid certificates"
        ));
    }

    Ok(())
}

fn sslmode_value(database_url: &str) -> Option<String> {
    connection_param_value(database_url, "sslmode").map(|value| value.to_ascii_lowercase())
}

fn sslrootcert_value(database_url: &str) -> Option<PathBuf> {
    connection_param_value(database_url, "sslrootcert").map(PathBuf::from)
}

fn normalize_connection_string_for_tokio_postgres(database_url: &str) -> Cow<'_, str> {
    if is_postgres_url(database_url) {
        normalize_postgres_url_for_tokio_postgres(database_url)
    } else {
        normalize_keyword_connection_string_for_tokio_postgres(database_url)
    }
}

fn connection_param_value(database_url: &str, key: &str) -> Option<String> {
    if is_postgres_url(database_url) {
        url_query_param_value(database_url, key)
    } else {
        keyword_connection_params(database_url)
            .ok()?
            .into_iter()
            .find(|param| param.key == key)
            .map(|param| param.value)
    }
}

fn is_postgres_url(database_url: &str) -> bool {
    database_url.starts_with("postgres://") || database_url.starts_with("postgresql://")
}

fn normalize_postgres_url_for_tokio_postgres(database_url: &str) -> Cow<'_, str> {
    let Some(query_start) = database_url.find('?') else {
        return Cow::Borrowed(database_url);
    };

    let fragment_start = database_url[query_start..]
        .find('#')
        .map(|offset| query_start + offset)
        .unwrap_or(database_url.len());
    let query = &database_url[query_start + 1..fragment_start];

    let mut changed = false;
    let mut params = vec![];

    for param in query.split('&') {
        let (key, value) = param.split_once('=').unwrap_or((param, ""));
        let decoded_key = percent_decode(key).to_ascii_lowercase();

        if decoded_key == "sslrootcert" {
            changed = true;
            continue;
        }

        if decoded_key == "sslmode" {
            let decoded_value = percent_decode(value).to_ascii_lowercase();
            if let Some(replacement) = normalized_sslmode_value(&decoded_value) {
                params.push(format!("{key}={replacement}"));
                changed = true;
                continue;
            }
        }

        params.push(param.to_string());
    }

    if !changed {
        return Cow::Borrowed(database_url);
    }

    let mut normalized = database_url[..query_start].to_string();
    if !params.is_empty() {
        normalized.push('?');
        normalized.push_str(&params.join("&"));
    }
    normalized.push_str(&database_url[fragment_start..]);

    Cow::Owned(normalized)
}

fn normalize_keyword_connection_string_for_tokio_postgres(database_url: &str) -> Cow<'_, str> {
    let Ok(params) = keyword_connection_params(database_url) else {
        return Cow::Borrowed(database_url);
    };
    let mut changed = false;
    let mut normalized_params = vec![];

    for param in params {
        if param.key == "sslrootcert" {
            changed = true;
            continue;
        }

        if param.key == "sslmode" {
            let normalized_value = param.value.to_ascii_lowercase();
            if let Some(replacement) = normalized_sslmode_value(&normalized_value) {
                normalized_params.push(format!("sslmode={replacement}"));
                changed = true;
                continue;
            }
        }

        normalized_params.push(database_url[param.range].to_string());
    }

    if changed {
        Cow::Owned(normalized_params.join(" "))
    } else {
        Cow::Borrowed(database_url)
    }
}

fn normalized_sslmode_value(value: &str) -> Option<&'static str> {
    match value {
        "verify-ca" | "verify-full" => Some("require"),
        "allow" => Some("prefer"),
        _ => None,
    }
}

fn url_query_param_value(database_url: &str, key: &str) -> Option<String> {
    let query_start = database_url.find('?')?;
    let fragment_start = database_url[query_start..]
        .find('#')
        .map(|offset| query_start + offset)
        .unwrap_or(database_url.len());
    let query = &database_url[query_start + 1..fragment_start];

    for param in query.split('&') {
        let (param_key, param_value) = param.split_once('=').unwrap_or((param, ""));
        if percent_decode(param_key).eq_ignore_ascii_case(key) {
            return Some(percent_decode(param_value));
        }
    }

    None
}

fn percent_decode(value: &str) -> String {
    let mut decoded = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let Some(high) = hex_value(bytes[index + 1]) else {
                    decoded.push(byte);
                    index += 1;
                    continue;
                };
                let Some(low) = hex_value(bytes[index + 2]) else {
                    decoded.push(byte);
                    index += 1;
                    continue;
                };
                decoded.push((high << 4) | low);
                index += 3;
            }
            _ => {
                decoded.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[derive(Debug, Eq, PartialEq)]
struct KeywordConnectionParam {
    key: String,
    value: String,
    range: Range<usize>,
}

fn keyword_connection_params(database_url: &str) -> Result<Vec<KeywordConnectionParam>, String> {
    let mut params = vec![];
    let mut offset = 0;

    while offset < database_url.len() {
        skip_ascii_whitespace(database_url, &mut offset);
        if offset >= database_url.len() {
            break;
        }

        let token_start = offset;
        let key_start = offset;
        take_until(database_url, &mut offset, |c| {
            c.is_ascii_whitespace() || c == '='
        });
        if key_start == offset {
            return Err("expected connection parameter key".to_string());
        }
        let key = database_url[key_start..offset].to_ascii_lowercase();

        skip_ascii_whitespace(database_url, &mut offset);
        eat_char(database_url, &mut offset, '=')?;
        skip_ascii_whitespace(database_url, &mut offset);
        let value = take_keyword_value(database_url, &mut offset)?;

        params.push(KeywordConnectionParam {
            key,
            value,
            range: token_start..offset,
        });
    }

    Ok(params)
}

fn skip_ascii_whitespace(value: &str, offset: &mut usize) {
    while let Some(ch) = value[*offset..].chars().next() {
        if !ch.is_ascii_whitespace() {
            break;
        }
        *offset += ch.len_utf8();
        if *offset >= value.len() {
            break;
        }
    }
}

fn take_until(value: &str, offset: &mut usize, stop: impl Fn(char) -> bool) {
    while let Some(ch) = value[*offset..].chars().next() {
        if stop(ch) {
            break;
        }
        *offset += ch.len_utf8();
        if *offset >= value.len() {
            break;
        }
    }
}

fn eat_char(value: &str, offset: &mut usize, expected: char) -> Result<(), String> {
    match value[*offset..].chars().next() {
        Some(ch) if ch == expected => {
            *offset += ch.len_utf8();
            Ok(())
        }
        Some(ch) => Err(format!("expected `{expected}` but found `{ch}`")),
        None => Err(format!("expected `{expected}` but found end of input")),
    }
}

fn take_keyword_value(value: &str, offset: &mut usize) -> Result<String, String> {
    if value[*offset..].starts_with('\'') {
        take_quoted_keyword_value(value, offset)
    } else {
        take_simple_keyword_value(value, offset)
    }
}

fn take_simple_keyword_value(value: &str, offset: &mut usize) -> Result<String, String> {
    let mut parsed = String::new();

    while let Some(ch) = value[*offset..].chars().next() {
        if ch.is_ascii_whitespace() {
            break;
        }

        *offset += ch.len_utf8();
        if ch == '\\' {
            if let Some(escaped) = value[*offset..].chars().next() {
                parsed.push(escaped);
                *offset += escaped.len_utf8();
            }
        } else {
            parsed.push(ch);
        }

        if *offset >= value.len() {
            break;
        }
    }

    if parsed.is_empty() {
        Err("expected connection parameter value".to_string())
    } else {
        Ok(parsed)
    }
}

fn take_quoted_keyword_value(value: &str, offset: &mut usize) -> Result<String, String> {
    let mut parsed = String::new();
    eat_char(value, offset, '\'')?;

    while let Some(ch) = value[*offset..].chars().next() {
        if ch == '\'' {
            *offset += ch.len_utf8();
            return Ok(parsed);
        }

        *offset += ch.len_utf8();
        if ch == '\\' {
            if let Some(escaped) = value[*offset..].chars().next() {
                parsed.push(escaped);
                *offset += escaped.len_utf8();
            }
        } else {
            parsed.push(ch);
        }

        if *offset >= value.len() {
            break;
        }
    }

    Err("unterminated quoted connection parameter value".to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        normalize_connection_string_for_tokio_postgres, PostgresTlsConfig, PostgresTlsMode,
    };

    #[test]
    fn missing_sslmode_keeps_tls_disabled() {
        let config = PostgresTlsConfig::from_database_url("postgres://localhost/chaindexing");

        assert_eq!(config.mode(), PostgresTlsMode::Disable);
    }

    #[test]
    fn require_sslmode_enables_required_tls() {
        let config = PostgresTlsConfig::from_database_url(
            "postgres://localhost/chaindexing?sslmode=require",
        );

        assert_eq!(config.mode(), PostgresTlsMode::Require);
    }

    #[test]
    fn verify_full_sslmode_maps_to_required_tls() {
        let config = PostgresTlsConfig::from_database_url(
            "postgres://localhost/chaindexing?sslmode=verify-full",
        );

        assert_eq!(config.mode(), PostgresTlsMode::Require);
    }

    #[test]
    fn unsupported_tokio_postgres_sslmodes_are_normalized_for_parsing() {
        assert_eq!(
            normalize_connection_string_for_tokio_postgres(
                "postgres://localhost/db?sslmode=verify-full&app=x"
            ),
            "postgres://localhost/db?sslmode=require&app=x"
        );
        assert_eq!(
            normalize_connection_string_for_tokio_postgres(
                "host=localhost user=postgres sslmode=allow"
            ),
            "host=localhost user=postgres sslmode=prefer"
        );
    }

    #[test]
    fn sslmode_parser_requires_a_parameter_boundary() {
        let config = PostgresTlsConfig::from_database_url(
            "postgres://localhost/chaindexing?application_name=not_sslmode=require",
        );

        assert_eq!(config.mode(), PostgresTlsMode::Disable);
    }

    #[test]
    fn keyword_sslmode_parser_allows_whitespace_around_equals() {
        let config =
            PostgresTlsConfig::from_database_url("host=localhost user=postgres sslmode = require");

        assert_eq!(config.mode(), PostgresTlsMode::Require);
    }

    #[test]
    fn keyword_sslmode_normalization_allows_whitespace_around_equals() {
        assert_eq!(
            normalize_connection_string_for_tokio_postgres(
                "host=localhost user=postgres sslmode = verify-full"
            ),
            "host=localhost user=postgres sslmode=require"
        );
    }

    #[test]
    fn sslrootcert_is_captured_and_removed_from_postgres_url() {
        let config = PostgresTlsConfig::from_database_url(
            "postgres://localhost/db?sslmode=require&sslrootcert=%2Ftmp%2Fca.pem&app=x",
        );

        assert_eq!(config.ca_cert_paths, vec![PathBuf::from("/tmp/ca.pem")]);
        assert_eq!(
            normalize_connection_string_for_tokio_postgres(
                "postgres://localhost/db?sslmode=require&sslrootcert=%2Ftmp%2Fca.pem&app=x"
            ),
            "postgres://localhost/db?sslmode=require&app=x"
        );
    }

    #[test]
    fn sslrootcert_is_captured_and_removed_from_keyword_connection_string() {
        let config = PostgresTlsConfig::from_database_url(
            "host=localhost sslrootcert = '/tmp/postgres ca.pem' sslmode = verify-full",
        );

        assert_eq!(
            config.ca_cert_paths,
            vec![PathBuf::from("/tmp/postgres ca.pem")]
        );
        assert_eq!(
            normalize_connection_string_for_tokio_postgres(
                "host=localhost sslrootcert = '/tmp/postgres ca.pem' sslmode = verify-full"
            ),
            "host=localhost sslmode=require"
        );
    }
}
