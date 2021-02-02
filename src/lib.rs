#[allow(unused_imports)]
pub mod error;
mod apple;
mod google;

use error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use yup_oauth2::ServiceAccountKey;

const APPLE_PROD_VERIFY_RECEIPT: &str = "https://buy.itunes.apple.com";
const APPLE_TEST_VERIFY_RECEIPT: &str = "https://sandbox.itunes.apple.com";

pub use apple::{AppleResponse, AppleUrls, apple_response, validate_apple_subscription};
pub use google::{GoogleResponse, google_response, validate_google_subscription};

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

#[derive(Default, Deserialize, Serialize, Clone, Debug)]
pub struct UnityPurchaseReceipt {
    #[serde(rename = "Store")]
    pub store: Platform,
    #[serde(rename = "Payload")]
    pub payload: String,
    #[serde(rename = "TransactionID")]
    pub transaction_id: String,
}

#[derive(Default, Deserialize, Serialize, Clone, Debug)]
pub struct PurchaseResponse {
    pub valid: bool,
}

#[async_trait]
pub trait Validator: Send + Sync {
    async fn validate(&self, receipt: &UnityPurchaseReceipt) -> Result<PurchaseResponse>;
}

pub struct UnityPurchaseValidator {
    secret: Option<String>,
    apple_urls: AppleUrls,
    service_account_key: Option<ServiceAccountKey>,
}

impl UnityPurchaseValidator {
    pub fn default() -> Self {
        Self {
            secret: None,
            apple_urls: AppleUrls {
                production: String::from(APPLE_PROD_VERIFY_RECEIPT),
                sandbox: String::from(APPLE_TEST_VERIFY_RECEIPT),
            },
            service_account_key: None,
        }
    }

    #[allow(clippy::missing_const_for_fn)]
    pub fn set_apple_secret(self, secret: String) -> Self {
        let mut new = self;
        new.secret = Some(secret);
        new
    }

    pub fn set_google_service_account_key<S: AsRef<[u8]>>(self, secret: S) -> Result<Self> {
        let mut new = self;
        new.service_account_key = serde_json::from_slice(secret.as_ref())?;
        Ok(new)
    }
}

#[async_trait]
impl Validator for UnityPurchaseValidator {
    async fn validate(&self, receipt: &UnityPurchaseReceipt) -> Result<PurchaseResponse> {

        slog::debug!(slog_scope::logger(), "purchase receipt validation";
            "store" => format!("{:?}",receipt.store),
            "transaction_id" => &receipt.transaction_id,
            "payload" => &receipt.payload,
        );

        match receipt.store {
            Platform::AppleAppStore => {
                let response = apple_response(receipt, &self.apple_urls, self.secret.as_ref()).await?;
                if response.status == 0 {
                    //apple returns latest_receipt_info if it is a renewable subscription
                    match response.latest_receipt {
                        Some(_) => validate_apple_subscription(response).await,
                        None => unimplemented!("validate consumable")
                    }
                } else {
                    Ok(PurchaseResponse{ valid: false })
                }
            }
            Platform::GooglePlay => {
                let google_data = google::GooglePlayData::from(&receipt.payload)?;
                let response = google_response(self.service_account_key.as_ref(),
                    &google_data.get_uri()?).await?;
                //TODO: figure out what the response is on invalid data. Should we check against an error? Is it a status code similar to apple?
                let sku_type = google_data.get_sku_details()?.sku_type;
                if sku_type == "subs" {
                    validate_google_subscription(response).await
                } else {
                    unimplemented!("validate consumable")
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

    fn new_for_test(test_url: &str) -> UnityPurchaseValidator {
        UnityPurchaseValidator {
            secret: Some(String::from("secret")),
            apple_urls: AppleUrls {
                production: String::from(test_url),
                sandbox: format!("{}/sb", test_url),
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

        let validator = new_for_test(url);

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

        let validator = new_for_test(url);

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

        let validator = new_for_test(url);

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

        let validator = new_for_test(url);

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

        assert!(!validate_google_subscription(google::google_response(None, url).await.unwrap()).await.unwrap().valid);
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

        assert!(validate_google_subscription(google::google_response(None, url).await.unwrap()).await.unwrap().valid);    }
}
