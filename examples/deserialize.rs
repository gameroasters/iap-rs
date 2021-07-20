use iap::UnityPurchaseReceipt;

fn main() {
    let raw_string = r#"{"Payload":"{\"json\":\"{\\\"orderId\\\":\\\"GPA.3349-5045-3269-66812\\\",\\\"packageName\\\":\\\"com.gameroasters.stack4\\\",\\\"productId\\\":\\\"com.gameroasters.s4.google.vip\\\",\\\"purchaseTime\\\":1625845453934,\\\"purchaseState\\\":0,\\\"purchaseToken\\\":\\\"pmkoeeioabjblfehjkdjdnig.AO-J1OzMAMUCsvmBp-Xg0_2Wa1zODlGjRnpjmTTHzcCLi1-tU1gndPCYGuKGz5fa-9pdWHBa816A6gTxYgFMJb9HTCIfADA894GEKVif4XsU5wBKCwLxw1w\\\",\\\"autoRenewing\\\":true,\\\"acknowledged\\\":false}\",\"signature\":\"V2sHMm4h5WcE8klV7lgA+f8sBlyg7rPyRsSojgJA3r3Uohh6MaclnFGSbz9hCtBnLueMNd0MoBnKX/yJWkz/ee3/wEeX4FT7KEzGIp/VLlqy8m+qB2nyYQnnRKaUmRRxVgjh74XTKV+myvNXihjMxPSGU4vB6xfdrapqBh59F3GYvCkmKLccWSMacOlhFmLZr+mTnzHFoAmcXGWkhSRbbFGoYV+r2Tt/EehIZq6FYBbvFxPl9ylWoPc8YYMCBMwL95fxRS3gT+G5ocRTeSPFMJLCXvD+Kywfct67QziE3nJTFYYpM5GyhYbno13bpTQ46P15H+hsAO8xsBL9f6P9Bg==\",\"skuDetails\":\"{\\\"productId\\\":\\\"com.gameroasters.s4.google.vip\\\",\\\"type\\\":\\\"subs\\\",\\\"price\\\":\\\"5,99\\u00a0\\u20ac\\\",\\\"price_amount_micros\\\":5990000,\\\"price_currency_code\\\":\\\"EUR\\\",\\\"title\\\":\\\"vip subscription (com.gameroasters.stack4 (unreviewed))\\\",\\\"description\\\":\\\"super human vip subscription\\\",\\\"subscriptionPeriod\\\":\\\"P1W\\\",\\\"skuDetailsToken\\\":\\\"AEuhp4K7yfvyfegjPzVzpt-E3dMDsdY-n9jAp4V4CF3t5p24MaIjgvPF7xVkEr_g3rvL\\\"}\"}","Store":"GooglePlay","TransactionID":"pmkoeeioabjblfehjkdjdnig.AO-J1OzMAMUCsvmBp-Xg0_2Wa1zODlGjRnpjmTTHzcCLi1-tU1gndPCYGuKGz5fa-9pdWHBa816A6gTxYgFMJb9HTCIfADA894GEKVif4XsU5wBKCwLxw1w"}"#;
    let receipt: UnityPurchaseReceipt = serde_json::from_str(raw_string).unwrap();
    println!("receipt: {:?}", receipt);
}
