mod apple;
#[allow(unused_imports)]
pub mod error;
mod google;

use async_trait::async_trait;
use error::Result;
use serde::{Deserialize, Serialize};
use yup_oauth2::ServiceAccountKey;

pub use apple::{
    apple_response, apple_response_with_urls, validate_apple_subscription, AppleResponse, AppleUrls,
};
pub use google::{
    google_response, google_response_with_uri, validate_google_subscription, GoogleResponse,
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

#[derive(Default)]
pub struct UnityPurchaseValidator<'a> {
    secret: Option<String>,
    apple_urls: AppleUrls<'a>,
    service_account_key: Option<ServiceAccountKey>,
}

impl UnityPurchaseValidator<'_> {
    #[allow(clippy::missing_const_for_fn)]
    pub fn set_apple_secret(self, secret: String) -> Self {
        let mut new = self;
        new.secret = Some(secret);
        new
    }

    pub fn set_google_service_account_key<S: AsRef<[u8]>>(self, secret: S) -> Result<Self> {
        let mut new = self;
        new.service_account_key = Some(google::get_service_account_key(secret)?);
        Ok(new)
    }
}

#[async_trait]
impl Validator for UnityPurchaseValidator<'_> {
    async fn validate(&self, receipt: &UnityPurchaseReceipt) -> Result<PurchaseResponse> {
        slog::debug!(slog_scope::logger(), "purchase receipt validation";
            "store" => format!("{:?}",receipt.store),
            "transaction_id" => &receipt.transaction_id,
            "payload" => &receipt.payload,
        );

        match receipt.store {
            Platform::AppleAppStore => {
                let response = apple::apple_response_with_urls(
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
                                google_response_with_uri(self.service_account_key.as_ref(), uri)
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
                google::google_response_with_uri(None, url.clone())
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
                google::google_response_with_uri(None, url.clone())
                    .await
                    .unwrap()
            )
            .unwrap()
            .valid
        );
    }
}
