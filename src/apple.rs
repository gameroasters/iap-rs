use super::{error::{Error::IoError, Result}, PurchaseResponse, UnityPurchaseReceipt};
use async_recursion::async_recursion;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use hyper::{body, Body, Client, Request};
use hyper_tls::HttpsConnector;

//https://developer.apple.com/documentation/appstorereceipts/status
const APPLE_STATUS_CODE_TEST: i32 = 21007;
const APPLE_PROD_VERIFY_RECEIPT: &str = "https://buy.itunes.apple.com";
const APPLE_TEST_VERIFY_RECEIPT: &str = "https://sandbox.itunes.apple.com";

pub struct AppleUrls<'a> {
    pub production: &'a str,
    pub sandbox: &'a str,
}

impl Default for AppleUrls<'_> {
    fn default() -> Self {
        AppleUrls {
            production: APPLE_PROD_VERIFY_RECEIPT,
            sandbox: APPLE_TEST_VERIFY_RECEIPT,
        }
    }
}

#[derive(Serialize)]
pub struct AppleRequest {
    #[serde(rename = "receipt-data")]
    pub receipt_data: String,
    pub password: String,
}

#[derive(Default, Serialize, Deserialize)]
pub struct AppleLatestReceipt {
    pub quantity: String,
    /// The time Apple customer support canceled a transaction, or the time an auto-renewable subscription plan was upgraded,
    /// in UNIX epoch time format, in milliseconds. This field is only present for refunded transactions. Use this time format for processing dates
    pub cancellation_date_ms: Option<String>,
    pub cancellation_reason: Option<String>,
    /// The time a subscription expires or when it will renew, in UNIX epoch time format, in milliseconds.
    /// Use this time format for processing dates.
    pub expires_date_ms: String,
    pub expires_date: String,
    pub original_purchase_date: String,
    pub product_id: String,
    pub purchase_date: String,
    pub transaction_id: String,
}

//see https://developer.apple.com/documentation/appstorereceipts/responsebody
#[derive(Default, Serialize, Deserialize)]
pub struct AppleResponse {
    pub status: i32,
    #[serde(rename = "is-retryable")]
    pub is_retryable: Option<bool>,
    pub environment: Option<String>,
    /// The latest Base64 encoded app receipt. Only returned for receipts that contain auto-renewable subscriptions. 
    #[serde(rename = "latest-receipt")]
    pub latest_receipt: Option<String>,
    /// An array that contains all in-app purchase transactions. This excludes transactions for consumable products
    /// that have been marked as finished by your app. Only returned for receipts that contain auto-renewable subscriptions.
    #[serde(rename = "latest-receipt-info")]
    pub latest_receipt_info: Option<Vec<AppleLatestReceipt>>,
}

/// Retrieves the responseBody data from Apple
pub async fn apple_response(
    receipt: &UnityPurchaseReceipt,
    password: Option<&String>,
) -> Result<AppleResponse> {
    apple_response_with_urls(receipt, &AppleUrls::default(), password).await
}

/// Response call with apple_urls parameter for tests
pub async fn apple_response_with_urls(
    receipt: &UnityPurchaseReceipt,
    apple_urls: &AppleUrls<'_>,
    password: Option<&String>,
) -> Result<AppleResponse> {
    let https = HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https);

    let password = password
        .cloned()
        .ok_or_else(|| IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "no apple secret has been set")))?;
    let request_body = serde_json::to_string(&AppleRequest {
        receipt_data: receipt.payload.clone(),
        password,
    })?;
    get_apple_response(&client, &request_body, apple_urls, true).await
}

pub async fn validate_apple_subscription(
    response: AppleResponse
) -> Result<PurchaseResponse> {
    
    let now = Utc::now().timestamp_millis();

    let valid = response
        .latest_receipt_info
        .as_ref()
        .and_then(|receipts| {
            receipts
                .iter()
                .max_by(|a, b| {
                    let a = a.expires_date_ms.parse::<i64>().unwrap_or_default();
                    let b = b.expires_date_ms.parse::<i64>().unwrap_or_default();

                    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Less)
                })
                .and_then(|receipt| {
                    receipt
                        .expires_date_ms
                        .parse::<i64>()
                        .map(|expiry_time| expiry_time > now)
                        .ok()
                })
        })
        .unwrap_or_default();

    Ok(PurchaseResponse { valid })
}

#[async_recursion]
async fn get_apple_response(
    client: &Client<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>,
    request_body: &str,
    apple_urls: &AppleUrls,
    prod: bool,
) -> Result<AppleResponse> {
    let req = Request::builder()
        .method("POST")
        .uri(format!(
            "{}/verifyReceipt",
            if prod {
                &apple_urls.production
            } else {
                &apple_urls.sandbox
            }
        ))
        .body(Body::from(request_body.to_owned()))?;

    let resp = client.request(req).await?;
    let buf = body::to_bytes(resp).await?;

    slog::debug!(
        slog_scope::logger(),
        "apple response: {}",
        String::from_utf8_lossy(&buf).replace("\n", "")
    );

    let response = serde_json::from_slice::<AppleResponse>(&buf)?;

    let latest_expires_date = response.latest_receipt_info.as_ref().and_then(|receipts| {
        receipts
            .iter()
            .max_by(|a, b| {
                let a = a.expires_date_ms.parse::<i64>().unwrap_or_default();
                let b = b.expires_date_ms.parse::<i64>().unwrap_or_default();
                a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Less)
            })
            .map(|receipt| receipt.expires_date.clone())
    });
    slog::info!(
        slog_scope::logger(), "apple response";
        "status" => &response.status,
        "latest_expires" => latest_expires_date,
    );

    if response.status == APPLE_STATUS_CODE_TEST {
        get_apple_response(client, request_body, apple_urls, false).await
    } else {
        Ok(response)
    }
}
