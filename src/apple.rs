#![allow(clippy::module_name_repetitions)]

use super::{
    error::{Error::IoError, Result},
    PurchaseResponse, UnityPurchaseReceipt,
};
use async_recursion::async_recursion;
use chrono::{DateTime, Utc};
use hyper::{body, Body, Client, Request};
use hyper_tls::HttpsConnector;
use serde::{Deserialize, Serialize};

/// <https://developer.apple.com/documentation/appstorereceipts/status>
const APPLE_STATUS_CODE_TEST: i32 = 21007;
/// <https://developer.apple.com/documentation/appstorereceipts/status>
const APPLE_STATUS_VALID: i32 = 0;
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
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct AppleLatestReceipt {
    pub quantity: Option<String>,
    /// The time Apple customer support canceled a transaction, or the time an auto-renewable subscription plan was upgraded,
    /// in UNIX epoch time format, in milliseconds. This field is only present for refunded transactions. Use this time format for processing dates
    pub cancellation_date_ms: Option<String>,
    pub cancellation_reason: Option<String>,
    /// The time a subscription expires or when it will renew, in UNIX epoch time format, in milliseconds.
    /// Use this time format for processing dates.
    pub expires_date_ms: Option<String>,
    pub expires_date: Option<String>,
    pub original_purchase_date: Option<String>,
    pub product_id: Option<String>,
    pub purchase_date: Option<String>,
    pub transaction_id: Option<String>,
}

/// See <https://developer.apple.com/documentation/appstorereceipts/responsebody> for more details on each field
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
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
    pub latest_receipt_info: Option<Vec<AppleLatestReceipt>>,
    /// A JSON representation of the receipt that was sent for verification
    pub receipt: Option<AppleReceipt>,
}

impl AppleResponse {
    #[must_use]
    /// Returns true if the receipt we are validating is from a subscription purchase
    pub fn is_subscription(&self, transaction_id: &str) -> bool {
        transaction_id.is_empty()
            || self
                .get_receipt(transaction_id)
                .filter(AppleInAppReceipt::is_subscription)
                .is_some()
    }

    #[must_use]
    /// Get the unique identifier of the product set in App Store Connect, ie: productIdentifier property of the `SKPayment` object
    pub fn get_product_id(&self, transaction_id: &str) -> Option<String> {
        self.get_receipt(transaction_id)
            .and_then(|receipt| receipt.product_id)
    }

    #[must_use]
    /// Get the receipt from `receipt.in_app` by the `transaction_id`
    pub fn get_receipt(&self, transaction_id: &str) -> Option<AppleInAppReceipt> {
        self.receipt
            .as_ref()
            .and_then(|receipt| receipt.get_transaction(transaction_id))
            .cloned()
    }

    #[must_use]
    /// Get the receipt with the latest expiration date from `receipt.in_app`
    pub fn get_latest_receipt(&self) -> Option<AppleInAppReceipt> {
        self.receipt
            .as_ref()
            .and_then(AppleReceipt::get_latest_receipt)
            .cloned()
    }
}

/// See <https://developer.apple.com/documentation/appstorereceipts/responsebody/receipt> for more details on each field
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct AppleReceipt {
    /// An array that contains the in-app purchase receipt fields for all in-app purchase transactions.
    pub in_app: Option<Vec<AppleInAppReceipt>>,
}

impl AppleReceipt {
    pub fn get_transaction(&self, transaction_id: &str) -> Option<&AppleInAppReceipt> {
        self.in_app.as_ref().and_then(|in_app| {
            in_app
                .iter()
                .find(|in_app| in_app.transaction_id.as_deref() == Some(transaction_id))
        })
    }

    pub fn get_latest_receipt(&self) -> Option<&AppleInAppReceipt> {
        self.in_app.as_ref().and_then(|in_app| {
            in_app.iter().max_by(|a, b| {
                let a = a
                    .expires_date_ms
                    .clone()
                    .unwrap_or_default()
                    .parse::<i64>()
                    .unwrap_or_default();
                let b = b
                    .expires_date_ms
                    .clone()
                    .unwrap_or_default()
                    .parse::<i64>()
                    .unwrap_or_default();

                a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Less)
            })
        })
    }
}

/// See <https://developer.apple.com/documentation/appstorereceipts/responsebody/receipt/in_app> for more details on each field
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct AppleInAppReceipt {
    /// The unique identifier of the product purchased. You provide this value when creating the product
    /// in App Store Connect, and it corresponds to the productIdentifier property of the SKPayment object stored in the
    /// transaction's payment property.
    pub product_id: Option<String>,
    /// A unique identifier for a transaction such as a purchase, restore, or renewal.
    pub transaction_id: Option<String>,
    pub expires_date_ms: Option<String>,
    pub expires_date: Option<String>,
}

impl AppleInAppReceipt {
    pub const fn is_subscription(&self) -> bool {
        self.expires_date_ms.is_some()
    }
}

/// Retrieves the responseBody data from Apple
/// # Errors
/// Will return an error if no apple secret is set in `password` or
/// if there is there is valid response from the `apple_urls` endpoints.
pub async fn fetch_apple_receipt_data(
    receipt: &UnityPurchaseReceipt,
    password: &str,
) -> Result<AppleResponse> {
    fetch_apple_receipt_data_with_urls(receipt, &AppleUrls::default(), Some(&password.to_string()))
        .await
}

/// Response call with `AppleUrls` parameter for tests
/// # Errors
/// Will return an error if no apple secret is set in `password` or
/// if there is there is valid response from the `apple_urls` endpoints.
pub async fn fetch_apple_receipt_data_with_urls(
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
    fetch_apple_response(
        &client,
        &request_body,
        apple_urls,
        &receipt.transaction_id,
        true,
    )
    .await
}

/// Simply validates based on whether or not the subscription's expiration has passed.
#[allow(clippy::must_use_candidate)]
pub fn validate_apple_subscription(
    response: &AppleResponse,
    transaction_id: &str,
    now: DateTime<Utc>,
) -> PurchaseResponse {
    let (valid, product_id) = if transaction_id.is_empty() {
        validate_expiration(now, response.get_latest_receipt())
    } else {
        let mut result = validate_expiration(now, response.get_receipt(transaction_id));

        let (valid, _) = result;
        if !valid {
            tracing::warn!(
                "Received an expired transaction_id: {}, attempting to find latest receipt",
                transaction_id
            );
            result = validate_expiration(now, response.get_latest_receipt());
        }

        result
    };

    PurchaseResponse { valid, product_id }
}

fn validate_expiration(
    now: DateTime<Utc>,
    in_app_receipt: Option<AppleInAppReceipt>,
) -> (bool, Option<String>) {
    in_app_receipt
        .and_then(|receipt| {
            receipt.expires_date_ms.as_ref().and_then(|expiry| {
                expiry
                    .parse::<i64>()
                    .map(|expiry_time| {
                        (
                            expiry_time > now.timestamp_millis(),
                            receipt.product_id.clone(),
                        )
                    })
                    .ok()
            })
        })
        .unwrap_or_default()
}

/// Validates that a package status is valid
#[allow(clippy::must_use_candidate)]
pub fn validate_apple_package(response: &AppleResponse, transaction_id: &str) -> PurchaseResponse {
    let product_id = response.get_product_id(transaction_id);
    let valid = response.status == APPLE_STATUS_VALID && product_id.is_some();

    PurchaseResponse {
        valid,
        product_id: response.get_product_id(transaction_id),
    }
}

#[async_recursion]
async fn fetch_apple_response(
    client: &Client<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>,
    request_body: &str,
    apple_urls: &AppleUrls,
    transaction_id: &str,
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

    tracing::debug!(
        "apple response: {}",
        String::from_utf8_lossy(&buf).replace('\n', "")
    );

    let response = serde_json::from_slice::<AppleResponse>(&buf)?;

    let latest_expires_date = response
        .get_receipt(transaction_id)
        .and_then(|receipt| receipt.expires_date);

    tracing::info!(target = "apple_response",
        product_id = ?response.get_product_id(transaction_id),
        is_subscription = %response.is_subscription(transaction_id),
        status = %&response.status,
        latest_expires_date = ?latest_expires_date,
    );

    if response.status == APPLE_STATUS_CODE_TEST {
        fetch_apple_response(client, request_body, apple_urls, transaction_id, false).await
    } else {
        Ok(response)
    }
}
