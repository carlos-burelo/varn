// Real-world HTTP REST Router & Middleware Pipeline in TypeScript

class Route {
    method: string;
    segments: string[];
    handler: (req: ReqContext) => ResContext;

    constructor(method: string, pattern: string, handler: (req: ReqContext) => ResContext) {
        this.method = method;
        this.handler = handler;
        this.segments = [];

        const rawSegs = pattern.split("/");
        for (let i = 0; i < rawSegs.length; i++) {
            const s = rawSegs[i];
            if (s.length > 0) {
                this.segments.push(s);
            }
        }
    }

    match(method: string, pathSegments: string[]): Record<string, string> | null {
        if (this.method !== "*" && this.method !== method) {
            return null;
        }
        if (this.segments.length !== pathSegments.length) {
            return null;
        }

        // Fast check: verify literal segments match first
        for (let i = 0; i < this.segments.length; i++) {
            const seg = this.segments[i];
            if (!seg.startsWith(":") && seg !== pathSegments[i]) {
                return null;
            }
        }

        const params: Record<string, string> = {};
        for (let i = 0; i < this.segments.length; i++) {
            const seg = this.segments[i];
            if (seg.startsWith(":")) {
                const pName = seg.slice(1);
                params[pName] = pathSegments[i];
            }
        }
        return params;
    }
}

class ReqContext {
    method: string;
    url: string;
    path: string;
    headers: Record<string, string>;
    params: Record<string, string>;
    query: Record<string, string>;
    user: string | null;

    constructor(method: string, url: string, headers: Record<string, string>) {
        this.method = method;
        this.url = url;
        this.headers = headers;
        this.params = {};
        this.query = {};
        this.user = null;

        const qIdx = url.indexOf("?");
        if (qIdx >= 0) {
            this.path = url.slice(0, qIdx);
            const qStr = url.slice(qIdx + 1);
            const pairs = qStr.split("&");
            for (let i = 0; i < pairs.length; i++) {
                const p = pairs[i];
                const eq = p.indexOf("=");
                if (eq > 0) {
                    this.query[p.slice(0, eq)] = p.slice(eq + 1);
                } else {
                    this.query[p] = "1";
                }
            }
        } else {
            this.path = url;
        }
    }
}

class ResContext {
    status: number;
    headers: Record<string, string>;
    body: string;

    constructor(status: number, body: string, headers: Record<string, string>) {
        this.status = status;
        this.body = body;
        this.headers = headers;
    }
}

class Router {
    routes: Route[];

    constructor() {
        this.routes = [];
    }

    add(method: string, pattern: string, handler: (req: ReqContext) => ResContext): void {
        this.routes.push(new Route(method, pattern, handler));
    }

    dispatch(req: ReqContext): ResContext {
        // 1. Auth Middleware
        const auth = req.headers["authorization"];
        if (auth == null || !auth.startsWith("Bearer ")) {
            return new ResContext(401, JSON.stringify({ error: "Unauthorized" }), { "Content-Type": "application/json" });
        }
        req.user = "user_" + auth.slice(7, 12);

        // 2. Route Matching
        const segs: string[] = [];
        const rawSegs = req.path.split("/");
        for (let i = 0; i < rawSegs.length; i++) {
            const s = rawSegs[i];
            if (s.length > 0) {
                segs.push(s);
            }
        }

        for (let i = 0; i < this.routes.length; i++) {
            const r = this.routes[i];
            const matchedParams = r.match(req.method, segs);
            if (matchedParams != null) {
                req.params = matchedParams;
                const h = r.handler;
                return h(req);
            }
        }

        return new ResContext(404, JSON.stringify({ error: "Not Found" }), { "Content-Type": "application/json" });
    }
}

// Setup 20 Production REST Routes
const router = new Router();

function jsonRes(status: number, data: any): ResContext {
    return new ResContext(status, JSON.stringify(data), {
        "Content-Type": "application/json",
        "X-Powered-By": "Node-Native-Router"
    });
}

router.add("GET", "/api/v1/health", (_req: ReqContext) => jsonRes(200, { status: "OK", uptime: 3600 }));
router.add("GET", "/api/v1/users", (req: ReqContext) => jsonRes(200, { users: ["Alice", "Bob"], page: req.query["page"] }));
router.add("POST", "/api/v1/users", (_req: ReqContext) => jsonRes(201, { created: true, id: 999 }));
router.add("GET", "/api/v1/users/:id", (req: ReqContext) => jsonRes(200, { id: req.params["id"], name: "User_" + req.params["id"] }));
router.add("PUT", "/api/v1/users/:id", (req: ReqContext) => jsonRes(200, { updated: true, id: req.params["id"] }));
router.add("DELETE", "/api/v1/users/:id", (_req: ReqContext) => jsonRes(204, { deleted: true }));
router.add("GET", "/api/v1/users/:id/orders", (req: ReqContext) => jsonRes(200, { userId: req.params["id"], count: 5 }));
router.add("POST", "/api/v1/users/:id/orders", (req: ReqContext) => jsonRes(201, { orderId: 888, userId: req.params["id"] }));
router.add("GET", "/api/v1/users/:id/orders/:orderId", (req: ReqContext) => jsonRes(200, { userId: req.params["id"], orderId: req.params["orderId"], status: "shipped" }));
router.add("GET", "/api/v1/products", (_req: ReqContext) => jsonRes(200, { products: [1, 2, 3] }));
router.add("GET", "/api/v1/products/:id", (req: ReqContext) => jsonRes(200, { productId: req.params["id"], inStock: true }));
router.add("POST", "/api/v1/products", (_req: ReqContext) => jsonRes(201, { created: true }));
router.add("GET", "/api/v1/categories", (_req: ReqContext) => jsonRes(200, { categories: ["A", "B"] }));
router.add("GET", "/api/v1/categories/:id/products", (req: ReqContext) => jsonRes(200, { categoryId: req.params["id"], items: 10 }));
router.add("GET", "/api/v1/cart/:userId", (req: ReqContext) => jsonRes(200, { cart: req.params["userId"], items: 2 }));
router.add("POST", "/api/v1/cart/:userId/items", (_req: ReqContext) => jsonRes(201, { added: true }));
router.add("DELETE", "/api/v1/cart/:userId/items/:itemId", (req: ReqContext) => jsonRes(200, { removed: req.params["itemId"] }));
router.add("POST", "/api/v1/checkout/:userId", (req: ReqContext) => jsonRes(200, { invoice: 12345, user: req.params["userId"] }));
router.add("GET", "/api/v1/analytics/overview", (_req: ReqContext) => jsonRes(200, { activeUsers: 1500, sales: 45000 }));
router.add("GET", "/api/v1/system/metrics", (_req: ReqContext) => jsonRes(200, { cpu: 12.5, memoryMb: 64 }));

// Generate 100,000 realistic requests
const samplePaths = [
    "/api/v1/health",
    "/api/v1/users?page=2",
    "/api/v1/users/42",
    "/api/v1/users/42/orders",
    "/api/v1/users/42/orders/101",
    "/api/v1/products/505",
    "/api/v1/categories/12/products",
    "/api/v1/cart/88/items/99",
    "/api/v1/analytics/overview",
    "/api/v1/system/metrics"
];

const methods = ["GET", "GET", "GET", "GET", "GET", "GET", "GET", "DELETE", "GET", "GET"];

const reqs: ReqContext[] = [];
for (let i = 0; i < 100000; i++) {
    const idx = i % 10;
    const hdrs: Record<string, string> = {
        "authorization": "Bearer token_secret_" + (i % 1000),
        "content-type": "application/json",
        "user-agent": "VarnBench/1.0"
    };
    reqs.push(new ReqContext(methods[idx], samplePaths[idx], hdrs));
}

// Execute 100,000 requests through middleware and routing pipeline
let successCount = 0;
let totalBytes = 0;

for (let i = 0; i < reqs.length; i++) {
    const res = router.dispatch(reqs[i]);
    if (res.status === 200 || res.status === 201) {
        successCount++;
    }
    totalBytes += res.body.length;
}

console.log(`HTTP Routing: Processed ${reqs.length} reqs, Success = ${successCount}, Total Response Bytes = ${totalBytes}`);
