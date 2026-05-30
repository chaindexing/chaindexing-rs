use std::{collections::HashMap, env, error::Error, sync::Arc};

use chaindexing::{
    ChaindexingRepo, HasRawQueryClient, InspectionResource, InspectionUi, InspectionUiQuery,
    LoadsDataWithRawQuery,
};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const DEFAULT_ADDR: &str = "127.0.0.1:8787";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let database_url = Arc::new(env::var("DATABASE_URL").map_err(|_| "DATABASE_URL must be set")?);
    let addr = env::var("CHAINDEXING_INSPECT_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
    let listener = TcpListener::bind(&addr).await?;

    println!("Chaindexing inspect UI listening on http://{addr}");

    loop {
        let (stream, _) = listener.accept().await?;
        let database_url = Arc::clone(&database_url);

        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, database_url).await {
                eprintln!("inspection request failed: {error}");
            }
        });
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    database_url: Arc<String>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut buffer = vec![0; 16 * 1024];
    let bytes_read = stream.read(&mut buffer).await?;

    if bytes_read == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let response = route_request(database_url.as_str(), &request).await;

    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await?;

    Ok(())
}

async fn route_request(database_url: &str, request: &str) -> String {
    let request_line = request.lines().next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or("/");

    if method != "GET" {
        return response(
            405,
            "application/json",
            &json!({ "error": "method not allowed" }).to_string(),
        );
    }

    let (path, query_string) = split_target(target);

    match path {
        "/" | "/index.html" => response(200, "text/html; charset=utf-8", InspectionUi::html()),
        path => {
            let Some(resource) = InspectionUi::resource_from_api_path(path) else {
                return response(
                    404,
                    "application/json",
                    &json!({ "error": "not found" }).to_string(),
                );
            };

            match load_resource(database_url, resource, query_string).await {
                Ok(rows) => response(
                    200,
                    "application/json",
                    &json!({ "rows": rows }).to_string(),
                ),
                Err(error) => response(
                    500,
                    "application/json",
                    &json!({ "error": error }).to_string(),
                ),
            }
        }
    }
}

async fn load_resource(
    database_url: &str,
    resource: InspectionResource,
    query_string: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let params = parse_query(query_string);
    let chain_id = parse_u64_param(&params, "chain_id", 1)?;
    let from_block_number = parse_u64_param(&params, "from_block_number", 0)?;
    let limit = parse_u32_param(&params, "limit", 100)?;
    let query = InspectionUiQuery::new(chain_id)
        .from_block_number(from_block_number)
        .limit(limit);
    let sql = InspectionUi::query(resource, query);

    let client = ChaindexingRepo::new(database_url)
        .get_client()
        .await
        .map_err(|error| error.to_string())?;

    ChaindexingRepo::load_data_list(&client, &sql)
        .await
        .map_err(|error| error.to_string())
}

fn split_target(target: &str) -> (&str, &str) {
    target.split_once('?').unwrap_or((target, ""))
}

fn parse_query(query_string: &str) -> HashMap<String, String> {
    query_string
        .split('&')
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            Some((percent_decode(key)?, percent_decode(value)?))
        })
        .collect()
}

fn parse_u64_param(
    params: &HashMap<String, String>,
    key: &str,
    default: u64,
) -> Result<u64, String> {
    params.get(key).map_or(Ok(default), |value| {
        value.parse::<u64>().map_err(|_| format!("{key} must be an unsigned integer"))
    })
}

fn parse_u32_param(
    params: &HashMap<String, String>,
    key: &str,
    default: u32,
) -> Result<u32, String> {
    params.get(key).map_or(Ok(default), |value| {
        value.parse::<u32>().map_err(|_| format!("{key} must be an unsigned integer"))
    })
}

fn percent_decode(value: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(value.len());
    let mut chars = value.as_bytes().iter().copied();

    while let Some(byte) = chars.next() {
        match byte {
            b'+' => bytes.push(b' '),
            b'%' => {
                let high = chars.next()?;
                let low = chars.next()?;
                let high = hex_value(high)?;
                let low = hex_value(low)?;
                bytes.push((high << 4) | low);
            }
            _ => bytes.push(byte),
        }
    }

    String::from_utf8(bytes).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn response(status: u16, content_type: &str, body: &str) -> String {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    };

    format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         Cache-Control: no-store\r\n\
         \r\n\
         {body}",
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_query_values() {
        let params = parse_query("chain_id=1&from_block_number=42&limit=100");

        assert_eq!(parse_u64_param(&params, "chain_id", 0), Ok(1));
        assert_eq!(parse_u64_param(&params, "from_block_number", 0), Ok(42));
        assert_eq!(parse_u32_param(&params, "limit", 0), Ok(100));
    }

    #[test]
    fn decodes_percent_encoded_values() {
        let params = parse_query("name=call%2Dtraces&space=a+b");

        assert_eq!(params.get("name"), Some(&"call-traces".to_string()));
        assert_eq!(params.get("space"), Some(&"a b".to_string()));
    }

    #[test]
    fn builds_http_response_with_content_length() {
        let http = response(200, "text/plain", "ok");

        assert!(http.starts_with("HTTP/1.1 200 OK"));
        assert!(http.contains("Content-Length: 2"));
        assert!(http.ends_with("\r\n\r\nok"));
    }
}
