/// Database connection handling with SSL/TLS support
use log::error;
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use postgres_openssl::MakeTlsConnector;
use tokio::time::Duration;
use url::Url;

/// Create SSL connector for PostgreSQL with custom CA certificate
///
/// This function sets up SSL/TLS connectivity for PostgreSQL connections,
/// including support for custom CA certificates (useful for cloud databases).
///
/// # Arguments
/// * `sslrootcert_path` - Path to the CA certificate file
///
/// # Returns
/// Result containing configured SSL connector or error message
pub fn create_ssl_connector(sslrootcert_path: &str) -> Result<MakeTlsConnector, String> {
    // Create SSL connector builder
    let mut builder =
        SslConnector::builder(SslMethod::tls()).map_err(|e| format!("SSL builder error: {}", e))?;

    // Load CA certificate for server verification
    builder
        .set_ca_file(sslrootcert_path)
        .map_err(|e| format!("Error loading CA cert: {}", e))?;

    // TEMPORARY: Disable certificate verification for self-signed certificates
    // In production, consider using proper certificate validation
    builder.set_verify(SslVerifyMode::NONE); // TEMPORARY FOR SELF-SIGNED CERTS

    Ok(MakeTlsConnector::new(builder.build()))
}

/// Execute database operations with automatic retry logic
///
/// This function provides robust database connectivity with automatic retries
/// for transient connection failures. It handles SSL connection setup,
/// connection pooling, and operation execution.
///
/// # Arguments
/// * `database_url` - PostgreSQL connection URL with SSL parameters
/// * `operation` - Async closure that performs the database operation
///
/// # Returns
/// Result indicating success or failure after all retries exhausted
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

        // Extract sslrootcert parameter and clean the URL
        let mut sslrootcert_path = None;
        let mut clean_params = Vec::new();
        for (key, value) in url.query_pairs() {
            if key == "sslrootcert" {
                sslrootcert_path = Some(value.to_string());
            } else {
                clean_params.push((key.into_owned(), value.into_owned()));
            }
        }

        // SSL root certificate is required for secure connections
        let sslrootcert_path = match sslrootcert_path {
            Some(path) => path,
            None => return Err("sslrootcert parameter missing".into()),
        };

        // Reconstruct URL without sslrootcert parameter (not recognized by tokio-postgres)
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

        // Create SSL connector with the extracted certificate path
        let connector = match create_ssl_connector(&sslrootcert_path) {
            Ok(c) => c,
            Err(e) => {
                error!("SSL connector error: {}", e);
                continue;
            }
        };

        // Attempt database connection
        match tokio_postgres::connect(&clean_database_url, connector).await {
            Ok((client, connection)) => {
                // Spawn connection handler in background
                tokio::spawn(async move {
                    if let Err(e) = connection.await {
                        error!("Connection error: {}", e);
                    }
                });

                // Execute the provided operation
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

    // All retry attempts exhausted
    Err("Max retries exceeded".into())
}
