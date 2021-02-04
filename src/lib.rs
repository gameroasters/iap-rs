#[allow(unused_imports)]
mod apple;
mod google;

pub mod error;

use async_trait::async_trait;
use error::Result;
use serde::{Deserialize, Serialize};
use yup_oauth2::ServiceAccountKey;

pub use apple::{
    get_apple_receipt_data, get_apple_receipt_data_with_urls, validate_apple_subscription,
    AppleResponse, AppleUrls,
};
pub use google::{
    get_google_receipt_data, get_google_receipt_data_with_uri, validate_google_subscription,
    GoogleResponse,
};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum Platform {
    AppleAppStore,
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
    #[serde(rename = "Store")]
    pub store: Platform,
    #[serde(rename = "Payload")]
    pub payload: String,
    #[serde(rename = "TransactionID")]
    pub transaction_id: String,
}

impl UnityPurchaseReceipt {
    /// Create a UnityPurchaseReceipt from the Json string delivered by Unity IAP.
    /// eg: "{ \"Store\": \"GooglePlay\", \"TransactionID\": \"<Txn ID>\", \"Payload\": \"<Payload>\" }"
    pub fn from(json_str: &str) -> Result<Self> {
        Ok(serde_json::from_str(json_str)?)
    }
}

#[derive(Default, Deserialize, Serialize, Clone, Debug)]
pub struct PurchaseResponse {
    pub valid: bool,
}

#[async_trait]
pub trait Validator: Send + Sync {
    async fn validate(&self, receipt: &UnityPurchaseReceipt) -> Result<PurchaseResponse>;
}

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
    /// Apple's shared secret required by their requestBody. See: https://developer.apple.com/documentation/appstorereceipts/requestbody
    secret: Option<String>,
    /// Should always be default unless we are using mock urls for offline unit tests.
    apple_urls: AppleUrls<'a>,
    /// The service account key required for Google's authentication.
    service_account_key: Option<ServiceAccountKey>,
}

impl UnityPurchaseValidator<'_> {
    /// Stores Apple's shared secret required by their requestBody. See: https://developer.apple.com/documentation/appstorereceipts/requestbody
    #[allow(clippy::missing_const_for_fn)]
    pub fn set_apple_secret(self, secret: String) -> Self {
        let mut new = self;
        new.secret = Some(secret);
        new
    }

    /// Stores Google's service account key. Takes the Json provided by Google's API with the following
    /// required fields:
    /// {
    ///     "private_key": "",
    ///     "client_email": "",
    ///     "token_uri": ""
    /// }
    pub fn set_google_service_account_key<S: AsRef<[u8]>>(self, secret: S) -> Result<Self> {
        let mut new = self;
        new.service_account_key = Some(google::get_service_account_key(secret)?);
        Ok(new)
    }
}

#[async_trait]
impl Validator for UnityPurchaseValidator<'_> {
    async fn validate(&self, receipt: &UnityPurchaseReceipt) -> Result<PurchaseResponse> {
        log::debug!(target: "validator", "store: {:?}, transaction_id: {}, payload: {}",
            receipt.store,
            &receipt.transaction_id,
            &receipt.payload,
        );

        match receipt.store {
            Platform::AppleAppStore => {
                let response = apple::get_apple_receipt_data_with_urls(
                    receipt,
                    &self.apple_urls,
                    self.secret.as_ref(),
                )
                .await?;
                if response.status == 0 {
                    //apple returns latest_receipt_info if it is a renewable subscription
                    match response.latest_receipt {
                        Some(_) => validate_apple_subscription(response),
                        None => unimplemented!("validate consumable"),
                    }
                } else {
                    Ok(PurchaseResponse { valid: false })
                }
            }
            Platform::GooglePlay => {
                //TODO: clean all of this up if async move evey makes its way to rust stable
                if let Ok((Ok(response_future), Ok(sku_type))) =
                    google::GooglePlayData::from(&receipt.payload).map(|data| {
                        (
                            data.get_uri().map(|uri| {
                                get_google_receipt_data_with_uri(
                                    self.service_account_key.as_ref(),
                                    uri,
                                )
                            }),
                            data.get_sku_details()
                                .map(|sku_details| sku_details.sku_type),
                        )
                    })
                {
                    if let Ok(response) = response_future.await {
                        if sku_type == "subs" {
                            validate_google_subscription(response)
                        } else {
                            unimplemented!("validate consumable")
                        }
                    } else {
                        Ok(PurchaseResponse { valid: false })
                    }
                } else {
                    Ok(PurchaseResponse { valid: false })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        apple::{AppleLatestReceipt, AppleResponse},
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
        let apple_response = AppleResponse {
            latest_receipt: Some(String::default()),
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
                .validate(&UnityPurchaseReceipt::default())
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
            expiry_time: Utc::now().timestamp_millis().to_string(),
            ..GoogleResponse::default()
        };

        let _m = mock("GET", "/test")
            .with_status(200)
            .with_body(&serde_json::to_string(&google_response).unwrap())
            .create();

        let url = &mockito::server_url();

        assert!(
            !validate_google_subscription(
                google::get_google_receipt_data_with_uri(None, url.clone())
                    .await
                    .unwrap()
            )
            .unwrap()
            .valid
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_google() {
        let google_response = GoogleResponse {
            expiry_time: (Utc::now() + Duration::days(1))
                .timestamp_millis()
                .to_string(),
            ..GoogleResponse::default()
        };
        let _m = mock("GET", "/test")
            .with_status(200)
            .with_body(&serde_json::to_string(&google_response).unwrap())
            .create();

        let url = &mockito::server_url();

        assert!(
            validate_google_subscription(
                google::get_google_receipt_data_with_uri(None, url.clone())
                    .await
                    .unwrap()
            )
            .unwrap()
            .valid
        );
    }
}
