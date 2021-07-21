//! iap is a rust library for verifying receipt information for purchases made through the Google Play Store or the Apple App Store.
//!
//! ## Current Features
//! - Validating receipt data received from [Unity's IAP plugin](https://docs.unity3d.com/Manual/UnityIAP.html) to verify subscriptions and if they are valid and not expired
//! - Helper functions to receive response data from Google/Apple for more granular error handling or validation
//!
//! ### Supported Transaction Types
//! - Subscriptions
//!
//! ### Coming Features
//! - Non-subscription purchase types
//! - Manual input of data for verification not received through Unity IAP
//!
//! ## Usage
//!
//! ### For simple validation of Unity IAP receipts
//! You can receive a `PurchaseResponse` which will simply tell you if a purchase is valid (and not expired if a subscription) by creating a `UnityPurchaseValidator`.
//!
//! ```ignore
//! use iap::*;
//!
//! const APPLE_SECRET: &str = "<APPLE SECRET>";
//! const GOOGLE_KEY: &str = "<GOOGLE KEY JSON>";
//!
//! #[tokio::main]
//! pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let validator = UnityPurchaseValidator::default()
//!         .set_apple_secret(APPLE_SECRET.to_string())
//!         .set_google_service_account_key(GOOGLE_KEY.to_string())?;
//!
//!     // RECEIPT_INPUT would be the Json string containing the store, transaction id, and payload
//!     // from Unity IAP. ie:
//!     // "{ \"Store\": \"GooglePlay\", \"TransactionID\": \"<Txn ID>\", \"Payload\": \"<Payload>\" }"
//!     let unity_receipt = UnityPurchaseReceipt::from(&std::env::var("RECEIPT_INPUT")?)?;
//!
//!     let response = validator.validate(&unity_receipt).await?;
//!
//!     println!("PurchaseResponse is valid: {}", response.valid);
//!
//!     Ok(())
//! }
//! ```
//!
//! If you wanted more granular control and access to the response from the store's endpoint, we provide helper functions to do so.
//!
//! For the Play Store:
//! ```rust
//! # use iap::*;
//! pub async fn validate(receipt: &UnityPurchaseReceipt) -> error::Result<PurchaseResponse> {
//!     let response = fetch_google_receipt_data(receipt, "<GOOGLE_KEY>").await?;
//!
//!     // debug or validate on your own with the data in the response
//!     println!("Expiry data: {:?}", response.expiry_time);
//!
//!     // or just simply validate the response
//!     validate_google_subscription(&response)
//! }
//! ```
//!
//! For the App Store:
//! ```rust
//! # use iap::*;
//! pub async fn validate(receipt: &UnityPurchaseReceipt) -> error::Result<PurchaseResponse> {
//!     let response = fetch_apple_receipt_data(receipt, "<APPLE_SECRET>").await?;
//!
//!     // was this purchase made in the production or sandbox environment
//!     println!("Environment: {}", response.environment.clone().unwrap());
//!
//!     Ok(validate_apple_subscription(&response))
//! }
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::pedantic)]
#![deny(missing_docs)]
#![deny(clippy::cargo)]
#![allow(clippy::multiple_crate_versions)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::panic)]
#![deny(clippy::perf)]
#![deny(clippy::nursery)]
#![deny(clippy::match_like_matches_macro)]

mod apple;
mod google;

pub mod error;

use async_trait::async_trait;
use error::Result;
use serde::{Deserialize, Serialize};
use yup_oauth2::ServiceAccountKey;

pub use apple::{
    fetch_apple_receipt_data, fetch_apple_receipt_data_with_urls, validate_apple_package,
    validate_apple_subscription, AppleResponse, AppleUrls,
};
pub use google::{
    fetch_google_receipt_data, fetch_google_receipt_data_with_uri, validate_google_package,
    validate_google_subscription, GoogleResponse, SkuType,
};

/// This is the platform on which the purchase that created the unity receipt was made.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum Platform {
    /// iOS and macOS
    AppleAppStore,
    /// Android
    GooglePlay,
}

impl Default for Platform {
    fn default() -> Self {
        Self::AppleAppStore
    }
}

/// Represents the deserialized contents of the Json string delivered by Unity IAP.
#[derive(Default, Deserialize, Serialize, Clone, Debug)]
pub struct UnityPurchaseReceipt {
    /// The platform on which the purchase that created the unity receipt was made.
    #[serde(rename = "Store")]
    pub store: Platform,
    /// The contents of the receipt to be processed by the validator
    #[serde(rename = "Payload")]
    pub payload: String,
    /// Transaction ID metadata
    #[serde(rename = "TransactionID")]
    pub transaction_id: String,
}

impl UnityPurchaseReceipt {
    /// Create a `UnityPurchaseReceipt` from the Json string delivered by Unity IAP.
    /// eg: "`{ \"Store\": \"GooglePlay\", \"TransactionID\": \"<Txn ID>\", \"Payload\": \"<Payload>\" }`"
    /// # Errors
    /// Will return an error if the `json_str` cannot be deserialized into a `UnityPurchaseReceipt`
    pub fn from(json_str: &str) -> Result<Self> {
        Ok(serde_json::from_str(json_str)?)
    }
}

/// A simple validation response returned by any of the validate methods which tells us if the receipt represents a valid purchase and/or active subscription.
#[derive(Default, Deserialize, Serialize, Clone, Debug)]
pub struct PurchaseResponse {
    /// Valid if true
    pub valid: bool,
    /// Product identifier
    pub product_id: Option<String>,
}

/// The base trait for implementing a validator. Mock Validators can be made for running local tests by implementing this trait.
#[async_trait]
pub trait Validator: Send + Sync {
    /// Called to perform the validation on whichever platform is described in the provided UnityPurchaseReceipt.
    async fn validate(&self, receipt: &UnityPurchaseReceipt) -> Result<PurchaseResponse>;
}

/// Trait which allows us to retrieve receipt data from an object's own secrets.
#[async_trait]
pub trait ReceiptDataFetcher {
    /// Similar to the helper function `crate::fetch_apple_receipt_data`, an associated function for pulling the response from owned secrets. x
    async fn fetch_apple_receipt_data(
        &self,
        receipt: &UnityPurchaseReceipt,
    ) -> Result<AppleResponse>;
    /// Similar to the helper function `crate::fetch_google_receipt_data`, an associated function for pulling the response from owned secrets.
    async fn fetch_google_receipt_data(
        &self,
        receipt: &UnityPurchaseReceipt,
    ) -> Result<(GoogleResponse, SkuType)>;
}

/// Convenience trait which combines `ReceiptDataFetcher` and `Validator` traits.
pub trait ReceiptValidator: ReceiptDataFetcher + Validator {}

/// Validator which stores our needed secrets for being able to authenticate against the stores' endpoints,
/// and performs our validation.
/// ```
/// use iap::UnityPurchaseValidator;
///
/// let validator = UnityPurchaseValidator::default()
///     .set_apple_secret("<APPLE_SECRET>".to_string())
///     .set_google_service_account_key("<GOOGLE_KEY>".to_string());
/// ```
#[derive(Default)]
pub struct UnityPurchaseValidator<'a> {
    /// Apple's shared secret required by their requestBody. See: <https://developer.apple.com/documentation/appstorereceipts/requestbody>
    pub secret: Option<String>,
    /// Should always be default unless we are using mock urls for offline unit tests.
    pub apple_urls: AppleUrls<'a>,
    /// The service account key required for Google's authentication.
    pub service_account_key: Option<ServiceAccountKey>,
}

impl ReceiptValidator for UnityPurchaseValidator<'_> {}

impl UnityPurchaseValidator<'_> {
    /// Stores Apple's shared secret required by their requestBody. See: <https://developer.apple.com/documentation/appstorereceipts/requestbody>
    #[allow(clippy::missing_const_for_fn)]
    #[allow(clippy::must_use_candidate)]
    pub fn set_apple_secret(self, secret: String) -> Self {
        tracing::info!("Setting apple secret");
        let mut new = self;
        new.secret = Some(secret);
        new
    }

    /// Stores Google's service account key. Takes the Json provided by Google's API with the following
    /// required fields:
    /// ```json
    /// {
    ///     "private_key": "",
    ///     "client_email": "",
    ///     "token_uri": ""
    /// }
    /// ```
    /// # Errors
    /// Will return an error if `S` cannot be deserialized into a `ServiceAccountKey`
    #[allow(clippy::must_use_candidate)]
    pub fn set_google_service_account_key<S: AsRef<[u8]>>(self, secret: S) -> Result<Self> {
        let mut new = self;
        new.service_account_key = Some(google::get_service_account_key(secret)?);
        Ok(new)
    }
}

#[async_trait]
impl Validator for UnityPurchaseValidator<'_> {
    async fn validate(&self, receipt: &UnityPurchaseReceipt) -> Result<PurchaseResponse> {
        tracing::debug!(
            "store: {:?}, transaction_id: {}, payload: {}",
            receipt.store,
            &receipt.transaction_id,
            &receipt.payload,
        );

        match receipt.store {
            Platform::AppleAppStore => {
                let response = apple::fetch_apple_receipt_data_with_urls(
                    receipt,
                    &self.apple_urls,
                    self.secret.as_ref(),
                )
                .await?;

                dbg!(&response);
                if response.status == 0 {
                    if response.is_subscription(&receipt.transaction_id) {
                        Ok(validate_apple_subscription(&response))
                    } else {
                        Ok(validate_apple_package(&response, &receipt.transaction_id))
                    }
                } else {
                    Ok(PurchaseResponse {
                        valid: false,
                        product_id: response.get_product_id(&receipt.transaction_id),
                    })
                }
            }
            Platform::GooglePlay => {
                //TODO: clean all of this up if async move evey makes its way to rust stable
                if let Ok((Ok(response_future), sku_type)) =
                    google::GooglePlayData::from(&receipt.payload).and_then(|data| {
                        data.get_sku_details().map(|sku_details| {
                            let sku_type = sku_details.sku_type;
                            (
                                data.get_uri(&sku_type).map(|uri| {
                                    fetch_google_receipt_data_with_uri(
                                        self.service_account_key.as_ref(),
                                        uri,
                                        Some(data),
                                    )
                                }),
                                sku_type,
                            )
                        })
                    })
                {
                    if let Ok(response) = response_future.await {
                        match sku_type {
                            google::SkuType::Subs => validate_google_subscription(&response),
                            google::SkuType::Inapp => Ok(validate_google_package(&response)),
                        }
                    } else {
                        Ok(PurchaseResponse {
                            valid: false,
                            product_id: None,
                        })
                    }
                } else {
                    //TODO:
                    Ok(PurchaseResponse {
                        valid: false,
                        product_id: None,
                    })
                }
            }
        }
    }
}

#[async_trait]
impl ReceiptDataFetcher for UnityPurchaseValidator<'_> {
    async fn fetch_apple_receipt_data(
        &self,
        receipt: &UnityPurchaseReceipt,
    ) -> Result<AppleResponse> {
        fetch_apple_receipt_data_with_urls(receipt, &self.apple_urls, self.secret.as_ref()).await
    }

    async fn fetch_google_receipt_data(
        &self,
        receipt: &UnityPurchaseReceipt,
    ) -> Result<(GoogleResponse, SkuType)> {
        let data = google::GooglePlayData::from(&receipt.payload)?;
        let sku_type = data.get_sku_details()?.sku_type;
        fetch_google_receipt_data_with_uri(
            self.service_account_key.as_ref(),
            data.get_uri(&sku_type)?,
            Some(data),
        )
        .await
        .map(|response| (response, sku_type))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        apple::{AppleInAppReceipt, AppleLatestReceipt, AppleReceipt, AppleResponse},
        google::{validate_google_subscription, GoogleResponse},
    };
    use chrono::{Duration, Utc};
    use mockito::mock;
    use serial_test::serial;

    fn new_for_test<'a>(prod_url: &'a str, sandbox_url: &'a str) -> UnityPurchaseValidator<'a> {
        UnityPurchaseValidator {
            secret: Some(String::from("secret")),
            apple_urls: AppleUrls {
                production: prod_url,
                sandbox: sandbox_url,
            },
            service_account_key: None,
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_sandbox_response() {
        let apple_response = AppleResponse {
            latest_receipt: Some(String::default()),
            latest_receipt_info: Some(vec![AppleLatestReceipt {
                expires_date_ms: (Utc::now() + Duration::days(1))
                    .timestamp_millis()
                    .to_string(),
                ..AppleLatestReceipt::default()
            }]),
            ..AppleResponse::default()
        };

        let _m1 = mock("POST", "/sb/verifyReceipt")
            .with_status(200)
            .with_body(&serde_json::to_string(&apple_response).unwrap())
            .create();

        let _m2 = mock("POST", "/verifyReceipt")
            .with_status(200)
            .with_body(r#"{"status": 21007}"#)
            .create();

        let url = &mockito::server_url();

        let sandbox = format!("{}/sb", url);
        let validator = new_for_test(url, &sandbox);

        assert!(
            validator
                .validate(&UnityPurchaseReceipt::default())
                .await
                .unwrap()
                .valid
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_invalid_receipt() {
        let now = Utc::now().timestamp_millis().to_string();
        let apple_response = AppleResponse {
            latest_receipt: Some(String::default()),
            receipt: Some(AppleReceipt {
                in_app: Some(vec![AppleInAppReceipt {
                    expires_date_ms: Some(now),
                    transaction_id: Some("txn".to_string()),
                    ..AppleInAppReceipt::default()
                }]),
            }),
            latest_receipt_info: Some(vec![AppleLatestReceipt {
                expires_date_ms: Utc::now().timestamp_millis().to_string(),
                ..AppleLatestReceipt::default()
            }]),
            ..AppleResponse::default()
        };

        let _m = mock("POST", "/verifyReceipt")
            .with_status(200)
            .with_body(&serde_json::to_string(&apple_response).unwrap())
            .create();

        let url = &mockito::server_url();

        let sandbox = format!("{}/sb", url);
        let validator = new_for_test(url, &sandbox);

        assert!(
            !validator
                .validate(&UnityPurchaseReceipt {
                    transaction_id: "txn".to_string(),
                    ..UnityPurchaseReceipt::default()
                })
                .await
                .unwrap()
                .valid
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_most_recent_receipt() {
        let now = Utc::now().timestamp_millis();
        let day = Duration::days(1).num_milliseconds();

        let latest_receipt_info = vec![
            AppleLatestReceipt {
                expires_date_ms: now.to_string(),
                ..AppleLatestReceipt::default()
            },
            AppleLatestReceipt {
                expires_date_ms: (now + day).to_string(),
                ..AppleLatestReceipt::default()
            },
            AppleLatestReceipt {
                expires_date_ms: (now - day).to_string(),
                ..AppleLatestReceipt::default()
            },
        ];

        let apple_response = AppleResponse {
            latest_receipt: Some(String::default()),
            latest_receipt_info: Some(latest_receipt_info),
            ..AppleResponse::default()
        };

        let _m = mock("POST", "/verifyReceipt")
            .with_status(200)
            .with_body(&serde_json::to_string(&apple_response).unwrap())
            .create();

        let url = &mockito::server_url();

        let sandbox = format!("{}/sb", url);
        let validator = new_for_test(url, &sandbox);

        assert!(
            validator
                .validate(&UnityPurchaseReceipt::default())
                .await
                .unwrap()
                .valid
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_apple_fail() {
        let _m = mock("POST", "/verifyReceipt")
            .with_status(200)
            .with_body(r#"{"status": 333}"#)
            .create();

        let url = &mockito::server_url();

        let sandbox = format!("{}/sb", url);
        let validator = new_for_test(url, &sandbox);

        assert!(
            !validator
                .validate(&UnityPurchaseReceipt::default())
                .await
                .unwrap()
                .valid
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_google_fail() {
        let google_response = GoogleResponse {
            expiry_time: Some(Utc::now().timestamp_millis().to_string()),
            ..GoogleResponse::default()
        };

        let _m = mock("GET", "/test")
            .with_status(200)
            .with_body(&serde_json::to_string(&google_response).unwrap())
            .create();

        let url = &mockito::server_url();

        assert!(
            !validate_google_subscription(
                &google::fetch_google_receipt_data_with_uri(None, url.clone(), None,)
                    .await
                    .unwrap()
            )
            .unwrap()
            .valid
        );
    }

    #[test]
    fn test_deserialize_apple() {
        let file = std::fs::read("res/test_apple.json").unwrap();
        let apple_response: AppleResponse = serde_json::from_slice(&file).unwrap();

        assert!(apple_response.latest_receipt.is_some());
        assert!(apple_response.latest_receipt_info.is_some());
        assert!(apple_response.environment.is_some());
    }

    #[test]
    fn test_deserialize_google() {
        let file = std::fs::read("res/test_google.json").unwrap();
        let _google_response: GoogleResponse = serde_json::from_slice(&file).unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn test_google() {
        let google_response = GoogleResponse {
            expiry_time: Some(
                (Utc::now() + Duration::days(1))
                    .timestamp_millis()
                    .to_string(),
            ),
            ..GoogleResponse::default()
        };
        let _m = mock("GET", "/test")
            .with_status(200)
            .with_body(&serde_json::to_string(&google_response).unwrap())
            .create();

        let url = &mockito::server_url();

        assert!(
            validate_google_subscription(
                &google::fetch_google_receipt_data_with_uri(None, url.clone(), None,)
                    .await
                    .unwrap()
            )
            .unwrap()
            .valid
        );
    }
}
