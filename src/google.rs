#![allow(clippy::module_name_repetitions)]

use super::{error, error::Result, PurchaseResponse, UnityPurchaseReceipt};
use chrono::Utc;
use hyper::{body, Body, Client, Request};
use hyper_tls::HttpsConnector;
use serde::{de::Error, Deserialize, Serialize};
use yup_oauth2::{ServiceAccountAuthenticator, ServiceAccountKey};

/// See <https://developers.google.com/android-publisher/api-ref/rest/v3/purchases.subscriptions#SubscriptionPurchase>
/// and <https://developers.google.com/android-publisher/api-ref/rest/v3/purchases.products#ProductPurchase> for details
/// on each field.
#[derive(Default, Serialize, Deserialize)]
pub struct GoogleResponse {
    /// Time at which the subscription will expire, in milliseconds since the Epoch.
    #[serde(rename = "expiryTimeMillis")]
    pub expiry_time: String,
    /// ISO 4217 currency code for the subscription price.
    #[serde(rename = "priceCurrencyCode")]
    pub price_currency_code: String,
    /// Price of the subscription, not including tax. Price is expressed in micro-units, where 1,000,000 micro-units represents one unit of the currency.
    #[serde(rename = "priceAmountMicros")]
    pub price_amount_micros: String,
    /// The order id of the latest recurring order associated with the purchase of the subscription.
    #[serde(rename = "orderId")]
    pub order_id: String,
    /// The type of purchase of the subscription. This field is only set if this purchase was not made using the standard in-app billing flow. Possible values are: 0. Test (i.e. purchased from a license testing account) 1. Promo (i.e. purchased using a promo code)
    #[serde(rename = "purchaseType")]
    pub purchase_type: Option<i64>,
}

#[derive(Serialize, Deserialize)]
pub struct GooglePlayData {
    pub json: String,
    pub signature: String,
    #[serde(rename = "skuDetails")]
    pub sku_details: String,
}

impl GooglePlayData {
    pub fn from(payload: &str) -> Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }

    pub fn get_uri(&self) -> Result<String> {
        let parameters: GooglePlayDataJson = serde_json::from_str(&self.json)?;

        log::debug!(
            "google purchase/receipt params, package: {}, productId: {}, token: {}",
            &parameters.package_name,
            &parameters.product_id,
            &parameters.token
        );

        Ok(format!(
            "https://androidpublisher.googleapis.com/androidpublisher/v3/applications/{}/purchases/subscriptions/{}/tokens/{}",
            parameters.package_name, parameters.product_id, parameters.token
        ))
    }

    pub fn get_sku_details(&self) -> Result<SkuDetails> {
        Ok(serde_json::from_str(&self.sku_details)?)
    }
}

#[derive(Deserialize)]
pub struct SkuDetails {
    #[serde(rename = "type")]
    pub sku_type: String,
}

#[derive(Serialize, Deserialize)]
pub struct GooglePlayDataJson {
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

/// Retrieves the response body from google
/// # Errors
/// Will return an error if authentication fails, if there is no response from the endpoint, or if the `payload` in the `UnityPurchaseReceipt` is malformed.
pub async fn get_google_receipt_data<S: AsRef<[u8]> + Send>(
    receipt: &UnityPurchaseReceipt,
    secret: S,
) -> Result<GoogleResponse> {
    let data = GooglePlayData::from(&receipt.payload)?;
    let uri = data.get_uri()?;

    let service_account_key = get_service_account_key(secret)?;

    get_google_receipt_data_with_uri(Some(&service_account_key), uri).await
}

/// Retrieves the google response with a specific uri, useful for running tests.
/// # Errors
/// Will return an error if authentication fails, if there is no response from the endpoint, or if the `payload` in the `UnityPurchaseReceipt` is malformed.
pub async fn get_google_receipt_data_with_uri(
    service_account_key: Option<&ServiceAccountKey>,
    uri: String,
) -> Result<GoogleResponse> {
    let https = HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https);

    log::debug!(
        "validate google parameters, service_account_key: {}, uri: {}",
        service_account_key.map_or(&"key not set".to_string(), |key| &key.client_email),
        uri.clone()
    );

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

    let response = client.request(req).await?;
    let buf = body::to_bytes(response).await?;
    let string = String::from_utf8(buf.to_vec())?.replace("\n", "");
    log::debug!("Google response: {}", &string);
    serde_json::from_slice(&buf).map_err(|err| {
        error::Error::SerdeError(serde_json::Error::custom(format!(
            "Failed to deserialize google response. Was the service account key set? Error message: {}", err)
        ))
    })
}

/// Simply validates based on whether or not the subscription's expiration has passed.
/// # Errors
/// Will return an error if the `expiry_time` in the response cannot be parsed as an `i64`
pub fn validate_google_subscription(response: &GoogleResponse) -> Result<PurchaseResponse> {
    let expiry_time = response.expiry_time.parse::<i64>()?;
    let now = Utc::now().timestamp_millis();
    let valid = expiry_time > now;

    log::info!("google receipt verification, valid: {}, now: {}, order_id: {}, expiry_time: {}, price_currency_code: {}, price_amount_micros: {}",
        valid,
        now,
        response.order_id,
        response.expiry_time,
        response.price_currency_code,
        response.price_amount_micros
    );

    Ok(PurchaseResponse { valid })
}

pub fn get_service_account_key<S: AsRef<[u8]>>(secret: S) -> Result<ServiceAccountKey> {
    Ok(serde_json::from_slice(secret.as_ref())?)
}
