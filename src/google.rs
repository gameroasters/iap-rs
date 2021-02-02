use super::PurchaseResponse;
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use warp::hyper::{body, Body, Client, Request};
use yup_oauth2::{ServiceAccountAuthenticator, ServiceAccountKey};

#[derive(Default, Serialize, Deserialize)]
pub struct GoogleResponse {
    #[serde(rename = "expiryTimeMillis")]
    pub expiry_time: String,
    #[serde(rename = "priceCurrencyCode")]
    pub price_currency_code: String,
    #[serde(rename = "priceAmountMicros")]
    pub price_amount_micros: String,
    #[serde(rename = "orderId")]
    pub order_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct GooglePlayParameters {
    pub json: String,
    pub signature: String,
    #[serde(rename = "skuDetails")]
    pub sku_details: String,
}

#[derive(Serialize, Deserialize)]
pub struct GooglePlayParametersJson {
    #[serde(rename = "packageName")]
    pub package_name: String,
    #[serde(rename = "productId")]
    pub product_id: String,
    #[serde(rename = "purchaseToken")]
    pub token: String,
    pub acknowledged: bool,
    #[serde(rename = "autoRenewing")]
    pub auto_renewing: bool,
    #[serde(rename = "purchaseTime")]
    pub purchase_time: i64,
    #[serde(rename = "orderId")]
    pub order_id: String,
    #[serde(rename = "purchaseState")]
    pub purchase_state: i64, //0 - unspecified, 1 - purchased, 2 - pending
}

pub async fn validate_google(
    client: &Client<hyper_tls::HttpsConnector<warp::hyper::client::HttpConnector>>,
    service_account_key: Option<&ServiceAccountKey>,
    uri: &str,
) -> Result<PurchaseResponse> {
    slog::debug!(slog_scope::logger(), "validate google parameters";
    "service_account_key" => service_account_key.map(|key| &key.client_email).unwrap_or(&"key not set".to_string()),
    "uri" => uri);

    let req = if let Some(key) = service_account_key {
        let authenticator = ServiceAccountAuthenticator::builder(key.clone())
            .build()
            .await?;

        let scopes = &["https://www.googleapis.com/auth/androidpublisher"];
        let auth_token = authenticator.token(scopes).await?;

        Request::builder()
            .method("GET")
            .header(
                "Authorization",
                format!("Bearer {}", auth_token.as_str()).as_str(),
            )
            .uri(uri)
            .body(Body::empty())
    } else {
        Request::builder()
            .method("GET")
            .uri(format!("{}/test", uri).as_str())
            .body(Body::empty())
    }?;

    let resp = client.request(req).await?;
    let buf = body::to_bytes(resp).await?;
    let string = String::from_utf8(buf.to_vec())?.replace("\n", "");
    slog::debug!(slog_scope::logger(), "Google response: {}", &string);
    let response: GoogleResponse = serde_json::from_slice(&buf).map_err(|err| {
        anyhow::Error::msg(format!(
            "Failed to deserialize google response. Was the service account key set? Error message: {}", err)
        )
    })?;

    let expiry_time = response.expiry_time.parse::<i64>()?;
    let now = Utc::now().timestamp_millis();
    let valid = expiry_time > now;

    slog::info!(slog_scope::logger(), "google receipt verification: {}", valid;
        "now" => now,
        "order_id" => response.order_id,
        "expiry_time" => response.expiry_time,
        "price_currency_code" => response.price_currency_code,
        "price_amount_micros" => response.price_amount_micros
    );

    Ok(PurchaseResponse { valid })
}

pub fn uri_from_payload(payload: &str) -> Result<String> {
    let parameters: GooglePlayParameters = serde_json::from_str(&payload)?;
    let parameters: GooglePlayParametersJson = serde_json::from_str(&parameters.json)?;

    slog::debug!(slog_scope::logger(), "google purchase/receipt params";
        "package" => &parameters.package_name,
        "productId" => &parameters.product_id,
        "token" => &parameters.token
    );

    Ok(format!(
        "https://androidpublisher.googleapis.com/androidpublisher/v3/applications/{}/purchases/subscriptions/{}/tokens/{}",
        parameters.package_name, parameters.product_id, parameters.token
    ))
}
