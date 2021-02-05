#![allow(clippy::module_name_repetitions)]

use super::{
    error::{Error::IoError, Result},
    PurchaseResponse, UnityPurchaseReceipt,
};
use async_recursion::async_recursion;
use chrono::Utc;
use hyper::{body, Body, Client, Request};
use hyper_tls::HttpsConnector;
use serde::{Deserialize, Serialize};

//https://developer.apple.com/documentation/appstorereceipts/status
const APPLE_STATUS_CODE_TEST: i32 = 21007;
const APPLE_PROD_VERIFY_RECEIPT: &str = "https://buy.itunes.apple.com";
const APPLE_TEST_VERIFY_RECEIPT: &str = "https://sandbox.itunes.apple.com";

/// Convenience struct for storing our production and sandbox URLs. Best practice is to attempt to verify
/// against production, and if that fails, to then request verification from the sandbox.
/// See: <https://developer.apple.com/documentation/appstorereceipts/verifyreceipt>
pub struct AppleUrls<'a> {
    /// By default, <https://buy.itunes.apple.com>
    pub production: &'a str,
    /// By default, <https://sandbox.itunes.apple.com>
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

/// See <https://developer.apple.com/documentation/appstorereceipts/responsebody/latest_receipt_info> for more details on each field.
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

/// See <https://developer.apple.com/documentation/appstorereceipts/responsebody> for more details on each field
#[derive(Default, Serialize, Deserialize)]
pub struct AppleResponse {
    /// Either 0 if the receipt is valid, or a status code if there is an error. The status code reflects the status of the app receipt as a whole.
    pub status: i32,
    /// An indicator that an error occurred during the request. A value of 1 indicates a temporary issue; retry validation for this receipt at a later time. A value of 0 indicates an unresolvable issue; do not retry validation for this receipt. Only applicable to status codes 21100-21199.
    #[serde(rename = "is-retryable")]
    pub is_retryable: Option<bool>,
    /// The environment for which the receipt was generated.
    /// Possible values: Sandbox, Production
    pub environment: Option<String>,
    /// The latest Base64 encoded app receipt. Only returned for receipts that contain auto-renewable subscriptions.
    pub latest_receipt: Option<String>,
    /// An array that contains all in-app purchase transactions. This excludes transactions for consumable products
    /// that have been marked as finished by your app. Only returned for receipts that contain auto-renewable subscriptions.
    #[serde(rename = "latest-receipt-info")]
    pub latest_receipt_info: Option<Vec<AppleLatestReceipt>>,
}

/// Retrieves the responseBody data from Apple
/// # Errors
/// Will return an error if no apple secret is set in `password` or
/// if there is there is valid response from the `apple_urls` endpoints.
pub async fn get_apple_receipt_data(
    receipt: &UnityPurchaseReceipt,
    password: &str,
) -> Result<AppleResponse> {
    get_apple_receipt_data_with_urls(receipt, &AppleUrls::default(), Some(&password.to_string()))
        .await
}

/// Response call with `AppleUrls` parameter for tests
/// # Errors
/// Will return an error if no apple secret is set in `password` or
/// if there is there is valid response from the `apple_urls` endpoints.
pub async fn get_apple_receipt_data_with_urls(
    receipt: &UnityPurchaseReceipt,
    apple_urls: &AppleUrls<'_>,
    password: Option<&String>,
) -> Result<AppleResponse> {
    let https = HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https);

    let password = password.cloned().ok_or_else(|| {
        IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no apple secret has been set",
        ))
    })?;
    let request_body = serde_json::to_string(&AppleRequest {
        receipt_data: receipt.payload.clone(),
        password,
    })?;
    get_apple_response(&client, &request_body, apple_urls, true).await
}

/// Simply validates based on whether or not the subscription's expiration has passed.
#[allow(clippy::must_use_candidate)]
pub fn validate_apple_subscription(response: &AppleResponse) -> PurchaseResponse {
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

    PurchaseResponse { valid }
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

    log::debug!(
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
    log::info!(
        "apple response, status: {}, latest_expires: {:?}",
        &response.status,
        latest_expires_date,
    );

    if response.status == APPLE_STATUS_CODE_TEST {
        get_apple_response(client, request_body, apple_urls, false).await
    } else {
        Ok(response)
    }
}
