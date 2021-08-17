#![allow(clippy::module_name_repetitions)]

use super::{error, error::Result, PurchaseResponse, UnityPurchaseReceipt};
use chrono::{DateTime, Utc};
use hyper::{body, Body, Client, Request};
use hyper_tls::HttpsConnector;
use serde::{de::Error, Deserialize, Serialize};
use yup_oauth2::{ServiceAccountAuthenticator, ServiceAccountKey};

/// See <https://developers.google.com/android-publisher/api-ref/rest/v3/purchases.subscriptions#SubscriptionPurchase>
/// and <https://developers.google.com/android-publisher/api-ref/rest/v3/purchases.products#ProductPurchase> for details
/// on each field.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct GoogleResponse {
    /// Time at which the subscription will expire, in milliseconds since the Epoch. Only set when it is a subscription
    #[serde(rename = "expiryTimeMillis")]
    pub expiry_time: Option<String>,
    /// ISO 4217 currency code for the subscription price.
    #[serde(rename = "priceCurrencyCode")]
    pub price_currency_code: Option<String>,
    /// Price of the subscription, not including tax. Price is expressed in micro-units, where 1,000,000 micro-units represents one unit of the currency.
    #[serde(rename = "priceAmountMicros")]
    pub price_amount_micros: Option<String>,
    /// The order id of the latest recurring order associated with the purchase of the subscription.
    #[serde(rename = "orderId")]
    pub order_id: String,
    /// The type of purchase of the subscription. This field is only set if this purchase was not made using the standard in-app billing flow. Possible values are: 0. Test (i.e. purchased from a license testing account) 1. Promo (i.e. purchased using a promo code)
    #[serde(rename = "purchaseType")]
    pub purchase_type: Option<i64>,
    #[serde(rename = "productId")]
    /// The inapp product SKU.
    pub product_id: Option<String>,
    #[serde(rename = "purchaseState")]
    /// The purchase state of the order. Possible values are: 0. Purchased 1. Canceled 2. Pending
    pub purchase_state: Option<u32>,
}

/// Metadata related to the purchase, used to populate the get request to google
#[derive(Serialize, Deserialize)]
pub struct GooglePlayData {
    /// JSON data which contains the url parameters for the get request
    pub json: String,
    ///
    pub signature: String,
    /// Contains the `SkuType`
    #[serde(rename = "skuDetails")]
    pub sku_details: String,
}

/// enum for differentiating between product purchases and subscriptions
#[derive(Deserialize)]
pub enum SkuType {
    /// Subscription
    #[serde(rename = "subs")]
    Subs,
    /// Product
    #[serde(rename = "inapp")]
    Inapp,
}

impl GooglePlayData {
    /// Construct the `GooglePlayData` from the `UnityPurchaseReceipt` payload
    pub fn from(payload: &str) -> Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }

    /// Construct the uri for the get request from the parameters in the json field
    pub fn get_uri(&self, sku_type: &SkuType) -> Result<String> {
        let parameters: GooglePlayDataJson = serde_json::from_str(&self.json)?;

        tracing::debug!(
            "google purchase/receipt params, package: {}, productId: {}, token: {}",
            &parameters.package_name,
            &parameters.product_id,
            &parameters.token,
        );

        match sku_type {
            SkuType::Subs => Ok(format!(
                "https://androidpublisher.googleapis.com/androidpublisher/v3/applications/{}/purchases/subscriptions/{}/tokens/{}",
                parameters.package_name, parameters.product_id, parameters.token
            )),
            SkuType::Inapp => Ok(format!(
                "https://androidpublisher.googleapis.com/androidpublisher/v3/applications/{}/purchases/products/{}/tokens/{}",
                parameters.package_name, parameters.product_id, parameters.token
            ))
        }
    }

    /// Extract the `SkuDetails`
    pub fn get_sku_details(&self) -> Result<SkuDetails> {
        Ok(serde_json::from_str(&self.sku_details)?)
    }
}

#[derive(Deserialize)]
pub struct SkuDetails {
    #[serde(rename = "type")]
    pub sku_type: SkuType,
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
    pub auto_renewing: Option<bool>,
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
pub async fn fetch_google_receipt_data<S: AsRef<[u8]> + Send>(
    receipt: &UnityPurchaseReceipt,
    secret: S,
) -> Result<GoogleResponse> {
    let data = GooglePlayData::from(&receipt.payload)?;
    let sku_details = data.get_sku_details()?;
    let uri = data.get_uri(&sku_details.sku_type)?;

    let service_account_key = get_service_account_key(secret)?;

    fetch_google_receipt_data_with_uri(Some(&service_account_key), uri, Some(data)).await
}

/// Retrieves the google response with a specific uri, useful for running tests.
/// # Errors
/// Will return an error if authentication fails, if there is no response from the endpoint, or if the `payload` in the `UnityPurchaseReceipt` is malformed.
pub async fn fetch_google_receipt_data_with_uri(
    service_account_key: Option<&ServiceAccountKey>,
    uri: String,
    data: Option<GooglePlayData>,
) -> Result<GoogleResponse> {
    let https = HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https);

    tracing::debug!(
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
    tracing::debug!("Google response: {}", &string);
    let mut response: GoogleResponse = serde_json::from_slice(&buf).map_err(|err| {
        error::Error::SerdeError(serde_json::Error::custom(format!(
            "Failed to deserialize google response. Was the service account key set? Error message: {}", err)
        ))
    })?;

    if response.product_id.is_none() {
        if let Some(data) = data {
            tracing::info!("Product id was not set in the response, getting from unity metadata");
            let parameters: GooglePlayDataJson = serde_json::from_str(&data.json)?;

            response.product_id = Some(parameters.product_id);
        }
    }

    Ok(response)
}

/// Simply validates based on whether or not the subscription's expiration has passed.
/// # Errors
/// Will return an error if the `expiry_time` in the response cannot be parsed as an `i64`
pub fn validate_google_subscription(
    response: &GoogleResponse,
    now: DateTime<Utc>,
) -> Result<PurchaseResponse> {
    let expiry_time = response
        .expiry_time
        .clone()
        .unwrap_or_default()
        .parse::<i64>()?;
    let now = now.timestamp_millis();
    let valid = expiry_time > now;

    tracing::info!("google receipt verification, valid: {}, now: {}, order_id: {}, expiry_time: {:?}, price_currency_code: {:?}, price_amount_micros: {:?}",
        valid,
        now,
        response.order_id,
        response.expiry_time,
        response.price_currency_code,
        response.price_amount_micros
    );

    Ok(PurchaseResponse {
        valid,
        product_id: response.product_id.clone(),
    })
}

#[must_use]
/// Simply validates product purchase
pub fn validate_google_package(response: &GoogleResponse) -> PurchaseResponse {
    let valid = response.purchase_state.filter(|i| *i == 0).is_some();
    tracing::info!(
        "google receipt verification, valid: {}, order_id: {}",
        valid,
        response.order_id,
    );

    PurchaseResponse {
        valid,
        product_id: response.product_id.clone(),
    }
}

pub fn get_service_account_key<S: AsRef<[u8]>>(secret: S) -> Result<ServiceAccountKey> {
    Ok(serde_json::from_slice(secret.as_ref())?)
}
