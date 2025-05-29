use log::error;
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use postgres_openssl::MakeTlsConnector;
use tokio::time::Duration;
use url::Url;

pub fn create_ssl_connector(sslrootcert_path: &str) -> Result<MakeTlsConnector, String> {
    let mut builder =
        SslConnector::builder(SslMethod::tls()).map_err(|e| format!("SSL builder error: {}", e))?;

    builder
        .set_ca_file(sslrootcert_path)
        .map_err(|e| format!("Error loading CA cert: {}", e))?;

    builder.set_verify(SslVerifyMode::NONE); // TEMPORARY FOR SELF-SIGNED CERTS

    Ok(MakeTlsConnector::new(builder.build()))
}

pub async fn execute_with_retry<F, Fut>(database_url: &str, operation: F) -> Result<(), String>
where
    F: Fn(tokio_postgres::Client) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<u64, tokio_postgres::Error>> + Send,
{
    const MAX_RETRIES: usize = 100;
    const WAIT_BETWEEN_RETRIES: u64 = 5;

    for attempt in 0..MAX_RETRIES {
        let url = match Url::parse(database_url) {
            Ok(url) => url,
            Err(e) => {
                error!("Attempt {}: URL parse error: {}", attempt + 1, e);
                continue;
            }
        };

        let mut sslrootcert_path = None;
        let mut clean_params = Vec::new();
        for (key, value) in url.query_pairs() {
            if key == "sslrootcert" {
                sslrootcert_path = Some(value.to_string());
            } else {
                clean_params.push((key.into_owned(), value.into_owned()));
            }
        }

        let sslrootcert_path = match sslrootcert_path {
            Some(path) => path,
            None => return Err("sslrootcert parameter missing".into()),
        };

        let mut clean_url = url.clone();
        clean_url.set_query(None);
        if !clean_params.is_empty() {
            let query = clean_params
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("&");
            clean_url.set_query(Some(&query));
        }
        let clean_database_url = clean_url.to_string();

        let connector = match create_ssl_connector(&sslrootcert_path) {
            Ok(c) => c,
            Err(e) => {
                error!("SSL connector error: {}", e);
                continue;
            }
        };

        match tokio_postgres::connect(&clean_database_url, connector).await {
            Ok((client, connection)) => {
                tokio::spawn(async move {
                    if let Err(e) = connection.await {
                        error!("Connection error: {}", e);
                    }
                });

                match operation(client).await {
                    Ok(_) => return Ok(()),
                    Err(e) => error!("Query error: {}", e),
                }
            }
            Err(e) => error!("Connection error: {}", e),
        }

        if attempt < MAX_RETRIES - 1 {
            tokio::time::sleep(Duration::from_secs(WAIT_BETWEEN_RETRIES)).await;
        }
    }

    Err("Max retries exceeded".into())
}
