![Build](https://github.com/gameroasters/iap-rs/workflows/Build/badge.svg) 
[![crates][s1]][l1]
[![Docs][s2]][l2]

[s1]: https://img.shields.io/crates/v/iap.svg
[l1]: https://crates.io/crates/iap
[s2]: https://docs.rs/iap/badge.svg
[l2]: https://docs.rs/iap

# iap

iap is a rust library for verifying receipt information for purchases made through the Google Play Store or the Apple App Store.

### Current Features
- Validating receipt data received from [Unity's IAP plugin](https://docs.unity3d.com/Manual/UnityIAP.html) to verify subscriptions and if they are valid and not expired
- Helper functions to receive response data from Google/Apple for more granular error handling or validation

#### Supported Transaction Types
- Subscriptions
- Consumables

#### Coming Features
- Non-subscription purchase types
- Manual input of data for verification not received through Unity IAP

### Usage

#### For simple validation of Unity IAP receipts
You can receive a `PurchaseResponse` which will simply tell you if a purchase is valid (and not expired if a subscription) by creating a `UnityPurchaseValidator`.

```rust
use iap::*;

const APPLE_SECRET: &str = "<APPLE SECRET>";
const GOOGLE_KEY: &str = "<GOOGLE KEY JSON>";

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let validator = UnityPurchaseValidator::default()
        .set_apple_secret(APPLE_SECRET.to_string())
        .set_google_service_account_key(GOOGLE_KEY.to_string())?;

    // RECEIPT_INPUT would be the Json string containing the store, transaction id, and payload
    // from Unity IAP. ie:
    // "{ \"Store\": \"GooglePlay\", \"TransactionID\": \"<Txn ID>\", \"Payload\": \"<Payload>\" }"
    let unity_receipt = UnityPurchaseReceipt::from(&std::env::var("RECEIPT_INPUT")?)?;

    let response = validator.validate(&unity_receipt).await?;

    println!("PurchaseResponse is valid: {}", response.valid);

    Ok(())
}
```

If you wanted more granular control and access to the response from the store's endpoint, we provide helper functions to do so.

For the Play Store:
```rust
pub async fn validate(receipt: &UnityPurchaseReceipt) -> error::Result<PurchaseResponse> {
    let response = fetch_google_receipt_data(receipt, "<GOOGLE_KEY>").await?;

    // debug or validate on your own with the data in the response
    println!("Expiry data: {}", response.expiry_time);

    // or just simply validate the response
    validate_google_subscription(&response)
}
```

For the App Store:
```rust
pub async fn validate(receipt: &UnityPurchaseReceipt) -> error::Result<PurchaseResponse> {
    let response = fetch_apple_receipt_data(receipt, "<APPLE_SECRET>").await?;

    // was this purchase made in the production or sandbox environment
    println!("Environment: {}", response.environment.clone().unwrap());

    Ok(validate_apple_subscription(&response))
}
```
