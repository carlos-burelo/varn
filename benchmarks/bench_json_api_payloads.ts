// Real-world JSON API Payloads in TypeScript: Complex Nested Object Serialization, Deserialization, and Hierarchy Traversal

const rawPayloads: string[] = [];

// 1. Generate 5,000 nested JSON documents
for (let i = 0; i < 5000; i++) {
    const orgId = "org_" + (i % 50);
    const doc = {
        "requestId": "req_" + i,
        "organization": {
            "id": orgId,
            "tier": (i % 2 === 0) ? "enterprise" : "pro",
            "settings": {
                "maxUsers": 1000,
                "features": ["sso", "audit_logs", "custom_roles", "api_access"]
            }
        },
        "user": {
            "id": "usr_" + (i % 500),
            "email": "user" + i + "@company.com",
            "roles": [
                { "role": "admin", "scope": "global" },
                { "role": "editor", "scope": "projects" }
            ],
            "metadata": {
                "lastLogin": "2026-08-21T12:00:00Z",
                "loginCount": (i % 30) + 1,
                "ip": "192.168.1." + (i % 254)
            }
        },
        "auditEvents": [
            { "event": "login", "status": "success", "ts": 1724241600 },
            { "event": "api_call", "endpoint": "/v1/data", "status": "success", "ts": 1724241605 }
        ]
    };

    rawPayloads.push(JSON.stringify(doc));
}

// 2. Parse, inspect, and aggregate nested JSON payloads
let totalAuditEvents = 0;
let totalLogins = 0;
let enterpriseCount = 0;
let totalFeaturesCount = 0;

for (let i = 0; i < rawPayloads.length; i++) {
    const parsed = JSON.parse(rawPayloads[i]);
    
    const org = parsed["organization"];
    if (org["tier"] === "enterprise") {
        enterpriseCount++;
    }
    
    const settings = org["settings"];
    const features = settings["features"] as any[];
    totalFeaturesCount += features.length;

    const user = parsed["user"];
    const meta = user["metadata"];
    totalLogins += Number(meta["loginCount"]);

    const events = parsed["auditEvents"] as any[];
    totalAuditEvents += events.length;
}

console.log("JSON API Payloads Benchmark:");
console.log("  Processed payloads: " + rawPayloads.length);
console.log("  Enterprise Orgs: " + enterpriseCount);
console.log("  Total User Logins: " + totalLogins);
console.log("  Total Features Mapped: " + totalFeaturesCount);
console.log("  Total Audit Events: " + totalAuditEvents);
