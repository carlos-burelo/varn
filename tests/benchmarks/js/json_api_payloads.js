const rawPayloads = [];
for (let i = 0; i < 5000; i++) {
    const doc = {
        requestId: `req_${i}`,
        organization: {
            id: `org_${i % 50}`,
            tier: (i % 2 === 0) ? 'enterprise' : 'pro',
            settings: { maxUsers: 1000, features: ['sso', 'audit_logs', 'custom_roles', 'api_access'] },
        },
        user: {
            id: `usr_${i % 500}`,
            email: `user${i}@company.com`,
            roles: [{ role: 'admin', scope: 'global' }, { role: 'editor', scope: 'projects' }],
            metadata: { lastLogin: '2026-08-21T12:00:00Z', loginCount: (i % 30) + 1, ip: `192.168.1.${i % 254}` },
        },
        auditEvents: [
            { event: 'login', status: 'success', ts: 1724241600 },
            { event: 'api_call', endpoint: '/v1/data', status: 'success', ts: 1724241605 },
        ],
    };
    rawPayloads.push(JSON.stringify(doc));
}
let totalAuditEvents = 0, totalLogins = 0, enterpriseCount = 0, totalFeaturesCount = 0;
for (let i = 0; i < rawPayloads.length; i++) {
    const parsed = JSON.parse(rawPayloads[i]);
    if (parsed.organization.tier === 'enterprise') enterpriseCount++;
    totalFeaturesCount += parsed.organization.settings.features.length;
    totalLogins += parsed.user.metadata.loginCount;
    totalAuditEvents += parsed.auditEvents.length;
}
console.log('JSON API Payloads Benchmark:');
console.log(`  Processed payloads: ${rawPayloads.length}`);
console.log(`  Enterprise Orgs: ${enterpriseCount}`);
console.log(`  Total User Logins: ${totalLogins}`);
console.log(`  Total Features Mapped: ${totalFeaturesCount}`);
console.log(`  Total Audit Events: ${totalAuditEvents}`);
