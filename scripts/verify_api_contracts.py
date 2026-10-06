#!/usr/bin/env python3
"""Validate an actual API/WS capture bundle against its served OpenAPI schemas."""
import json
import sys
from jsonschema import Draft202012Validator


def main():
    with open(sys.argv[1], encoding="utf-8") as stream:
        bundle = json.load(stream)
    spec = bundle["openapi"]
    assert spec["openapi"] == "3.1.0"
    components = spec["components"]
    for name, schema in components["schemas"].items():
        Draft202012Validator.check_schema(schema)
    for case in bundle["cases"]:
        schema = {"$schema": spec["jsonSchemaDialect"], "$ref": case["schema_ref"],
                  "components": components}
        validator = Draft202012Validator(schema)
        valid = validator.is_valid(case["payload"])
        if valid != case.get("valid", True):
            # Do not echo payloads, which could include untrusted request data.
            raise AssertionError("contract mismatch: " + case["label"])
    writes = [(path, method) for path, item in spec["paths"].items()
              for method in item if method == "post"]
    assert set(writes) == {("/v1/quote", "post"), ("/v1/prove", "post"), ("/v1/webhook", "post")}
    for path, method in writes:
        auth = "WebhookHmac" if path == "/v1/webhook" else "OperatorBearer"
        assert spec["paths"][path][method]["security"] == [{auth: []}]
    webhook_auth = spec["components"]["securitySchemes"]["WebhookHmac"]
    assert webhook_auth["type"] == "apiKey" and webhook_auth["name"] == "X-Webhook-Signature"
    for path, item in spec["paths"].items():
        if "get" in item:
            assert item["get"]["security"] == []
    print("PASS: served REST/WS captures and negative cases match OpenAPI schemas.")


if __name__ == "__main__":
    main()
