import os, sys
_here = os.path.dirname(os.path.abspath(__file__))
sys.path = [p for p in sys.path if os.path.abspath(p) != _here]
import json

raw = []
for i in range(5000):
    doc = {
        "requestId": f"req_{i}",
        "organization": {
            "id": f"org_{i % 50}",
            "tier": "enterprise" if i % 2 == 0 else "pro",
            "settings": {"maxUsers": 1000, "features": ["sso", "audit_logs", "custom_roles", "api_access"]},
        },
        "user": {
            "id": f"usr_{i % 500}",
            "email": f"user{i}@company.com",
            "roles": [
                {"role": "admin", "scope": "global"},
                {"role": "editor", "scope": "projects"},
            ],
            "metadata": {
                "lastLogin": "2026-08-21T12:00:00Z",
                "loginCount": (i % 30) + 1,
                "ip": f"192.168.1.{i % 254}",
            },
        },
        "auditEvents": [
            {"event": "login", "status": "success", "ts": 1724241600},
            {"event": "api_call", "endpoint": "/v1/data", "status": "success", "ts": 1724241605},
        ],
    }
    raw.append(json.dumps(doc, separators=(chr(44), chr(58))))

total_audit = total_logins = enterprise = total_features = 0
for text in raw:
    parsed = json.loads(text)
    if parsed["organization"]["tier"] == "enterprise":
        enterprise += 1
    total_features += len(parsed["organization"]["settings"]["features"])
    total_logins += parsed["user"]["metadata"]["loginCount"]
    total_audit += len(parsed["auditEvents"])

print("JSON API Payloads Benchmark:")
print(f"  Processed payloads: {len(raw)}")
print(f"  Enterprise Orgs: {enterprise}")
print(f"  Total User Logins: {total_logins}")
print(f"  Total Features Mapped: {total_features}")
print(f"  Total Audit Events: {total_audit}")
