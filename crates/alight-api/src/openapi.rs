use alight_types::*;
use schemars::{JsonSchema, schema_for};
use serde_json::{Map, Value, json};

fn rewrite(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get_mut("$ref")
                && reference.starts_with("#/$defs/")
            {
                *reference = reference.replacen("#/$defs/", "#/components/schemas/", 1);
            }
            for value in map.values_mut() {
                rewrite(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                rewrite(value);
            }
        }
        _ => {}
    }
}
fn add<T: JsonSchema>(schemas: &mut Map<String, Value>) {
    let mut value = schema_for!(T).to_value();
    if let Some(map) = value.as_object_mut() {
        map.remove("$schema");
        if let Some(Value::Object(defs)) = map.remove("$defs") {
            for (name, mut schema) in defs {
                rewrite(&mut schema);
                schemas.insert(name, schema);
            }
        }
    }
    rewrite(&mut value);
    schemas.insert(T::schema_name().into_owned(), value);
}
fn response(name: &str) -> Value {
    json!({"description":"Typed source-scoped response","content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{name}")}}}})
}
fn operation(name: &str, write: bool, status: &str) -> Value {
    let mut responses = Map::new();
    responses.insert(status.into(), response(name));
    for code in [
        "400", "401", "403", "404", "405", "408", "409", "413", "414", "429", "503",
    ] {
        responses.insert(code.into(), response("ApiErrorResponse"));
    }
    json!({"security":if write {json!([{"OperatorBearer":[]}])}else{json!([])},"responses":responses})
}
fn query(name: &str, schema: Value, required: bool) -> Value {
    json!({"name":name,"in":"query","required":required,"schema":schema})
}
/// OpenAPI 3.1 with schemas derived from the exact serde contracts used by the handlers.
pub fn document() -> Value {
    let mut schemas = Map::new();
    add::<ApiErrorResponse>(&mut schemas);
    add::<ApiHealth>(&mut schemas);
    add::<ApiClock>(&mut schemas);
    add::<ApiTelemetry>(&mut schemas);
    add::<ObserverHealthPage>(&mut schemas);
    add::<CurvePage>(&mut schemas);
    add::<QuoteServiceRequest>(&mut schemas);
    add::<QuotePreview>(&mut schemas);
    add::<ForecastEntry>(&mut schemas);
    add::<ProveRequest>(&mut schemas);
    add::<ProveReport>(&mut schemas);
    add::<ProvePage>(&mut schemas);
    add::<LedgerPage>(&mut schemas);
    add::<LedgerVerification>(&mut schemas);
    add::<TapePage>(&mut schemas);
    add::<ApiStreamSnapshot>(&mut schemas);
    add::<Alert>(&mut schemas);
    add::<ForecastLedgerExport>(&mut schemas);
    add::<WorkbenchEvidence>(&mut schemas);
    add::<DiagnosticsPage>(&mut schemas);
    add::<WebhookReceipt>(&mut schemas);
    add::<CanaryEvidencePage>(&mut schemas);
    add::<ProveCanaryPage>(&mut schemas);
    add::<BrowserLedgerPage>(&mut schemas);
    // Boundary bounds complement the serde shape; semantic/model gates still run in handlers.
    schemas["ProveRequest"]["properties"]["n"]["minimum"] = json!(1);
    schemas["ProveRequest"]["properties"]["n"]["maximum"] = json!(400);
    schemas["ProveRequest"]["properties"]["request_id"]["pattern"] = json!("^[A-Za-z0-9_-]{1,64}$");
    schemas["QuoteServiceRequest"]["properties"]["ttl_s"]["minimum"] = json!(1);
    schemas["QuoteServiceRequest"]["properties"]["ttl_s"]["maximum"] = json!(86400);
    schemas["ModelQuoteRequest"]["properties"]["candidates"]["minItems"] = json!(1);
    schemas["ModelQuoteRequest"]["properties"]["candidates"]["maxItems"] = json!(81);
    let mut paths = Map::new();
    let mut webhook = operation("WebhookReceipt", false, "200");
    webhook["security"] = json!([{"WebhookHmac":[]}]);
    webhook["description"] = json!(
        "Live/observe only, default disabled. Exact body HMAC-SHA256 in X-Webhook-Signature (64 hex characters), 65536-byte cap, owned signatures only. Unknown identities/index scopes remain incomplete. Valid unowned deliveries are ignored."
    );
    webhook["requestBody"] = json!({"required":true,"content":{"application/json":{"schema":{"type":"object","required":["signature","slot","status"],"properties":{"signature":{"type":"string","maxLength":90},"slot":{"type":"integer","minimum":0},"status":{"type":"string"}}}}}});
    paths.insert("/v1/webhook".into(), json!({"post":webhook}));
    for (path, name) in [
        ("/v1/health", "ApiHealth"),
        ("/v1/clock", "ApiClock"),
        ("/v1/leaders", "ApiTelemetry"),
        ("/v1/observers", "ObserverHealthPage"),
        ("/v1/ledger/verify", "LedgerVerification"),
        ("/v1/workbench", "WorkbenchEvidence"),
        ("/v1/diagnostics", "DiagnosticsPage"),
    ] {
        paths.insert(path.into(), json!({"get":operation(name,false,"200")}));
    }
    let mut read = operation("QuotePreview", false, "200");
    read["parameters"] = json!([{"name":"request","in":"query","required":true,"description":"URL-encoded QuoteServiceRequest JSON; entire query at most 8192 bytes","content":{"application/json":{"schema":{"$ref":"#/components/schemas/QuoteServiceRequest"}}}}]);
    let mut write = operation("ForecastEntry", true, "201");
    write["requestBody"] = json!({"required":true,"content":{"application/json":{"schema":{"$ref":"#/components/schemas/QuoteServiceRequest"}}}});
    paths.insert("/v1/quote".into(), json!({"get":read,"post":write}));
    let mut prove = operation("ProveReport", true, "200");
    prove["responses"]["202"] = response("ProveReport");
    prove["requestBody"] = json!({"required":true,"content":{"application/json":{"schema":{"$ref":"#/components/schemas/ProveRequest"}}}});
    paths.insert("/v1/prove".into(), json!({"post":prove}));
    let mut report = operation("ProveReport", false, "200");
    report["parameters"] = json!([{"name":"id","in":"path","required":true,"schema":{"type":"string","maxLength":100}}]);
    paths.insert("/v1/prove/{id}".into(), json!({"get":report}));
    for (path, name, max) in [
        ("/v1/prove/{id}/canaries", "ProveCanaryPage", 100),
        ("/v1/canaries/{id}/observations", "CanaryEvidencePage", 128),
    ] {
        let mut op = operation(name, false, "200");
        op["parameters"] = json!([{"name":"id","in":"path","required":true,"schema":{"type":"string","maxLength":max}}]);
        paths.insert(path.into(), json!({"get":op}));
    }
    for (path, name, parameters) in [
        (
            "/v1/curve",
            "CurvePage",
            vec![
                query("regime", json!({"type":"string","maxLength":128}), false),
                query(
                    "limit",
                    json!({"type":"integer","minimum":1,"maximum":1000,"default":243}),
                    false,
                ),
            ],
        ),
        (
            "/v1/proves",
            "ProvePage",
            vec![query(
                "limit",
                json!({"type":"integer","minimum":1,"maximum":100,"default":20}),
                false,
            )],
        ),
        (
            "/v1/ledger",
            "LedgerPage",
            vec![
                query(
                    "after",
                    json!({"$ref":"#/components/schemas/DecimalU64","default":"0"}),
                    false,
                ),
                query(
                    "limit",
                    json!({"type":"integer","minimum":1,"maximum":100,"default":50}),
                    false,
                ),
            ],
        ),
        (
            "/v1/tape",
            "TapePage",
            vec![
                query("from", json!({"type":"string","format":"date-time"}), false),
                query(
                    "through",
                    json!({"type":"string","format":"date-time"}),
                    false,
                ),
                query(
                    "limit",
                    json!({"type":"integer","minimum":1,"maximum":1000,"default":100}),
                    false,
                ),
            ],
        ),
    ] {
        let mut op = operation(name, false, "200");
        op["parameters"] = json!(parameters);
        paths.insert(path.into(), json!({"get":op}));
    }
    paths.insert("/v1/openapi.json".into(),json!({"get":{"security":[],"responses":{"200":{"description":"OpenAPI 3.1 document","content":{"application/json":{"schema":{"type":"object"}}}},"429":response("ApiErrorResponse")}}}));
    let mut payloads = operation("BrowserLedgerPage", false, "200");
    payloads["parameters"] = json!([
        query(
            "after",
            json!({"$ref":"#/components/schemas/DecimalU64","default":"0"}),
            false
        ),
        query(
            "limit",
            json!({"type":"integer","minimum":1,"maximum":100,"default":50}),
            false
        )
    ]);
    paths.insert("/v1/ledger/payloads".into(), json!({"get":payloads}));
    paths.insert("/v1/stream".into(),json!({"get":{"security":[],"description":"Read-only source-scoped snapshots each second; 32 connections maximum, 1 MiB output cap, 4 KiB input cap, slow clients disconnect. Text/binary controls are rejected.","responses":{"101":{"description":"WebSocket upgrade"},"429":response("ApiErrorResponse")},"x-websocket-message":{"$ref":"#/components/schemas/ApiStreamSnapshot"}}}));
    json!({"openapi":"3.1.0","jsonSchemaDialect":"https://json-schema.org/draft/2020-12/schema","info":{"title":"Alight API","version":"1","description":"Canary landing forecasts with explicit live/sim/replay evidence. Public reads never sign, send or persist forecasts."},"paths":paths,"components":{"schemas":schemas,"securitySchemes":{"WebhookHmac":{"type":"apiKey","in":"header","name":"X-Webhook-Signature","description":"64 hexadecimal characters: HMAC-SHA256 of exact body bytes using SOLAMI_WEBHOOK_SECRET"},"OperatorBearer":{"type":"http","scheme":"bearer","description":"Server-side ALIGHT_OPERATOR_KEY. Never put it in a URL."}}}})
}
