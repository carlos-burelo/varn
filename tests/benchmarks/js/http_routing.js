class Route {
  constructor(method, pattern, handler) {
    this.method = method;
    this.handler = handler;
    this.segments = [];
    for (const s of pattern.split("/")) {
      if (s.length > 0) this.segments.push(s);
    }
  }
  match(method, pathSegments) {
    if (this.method !== "*" && this.method !== method) return null;
    if (this.segments.length !== pathSegments.length) return null;
    for (let i = 0; i < this.segments.length; i++) {
      const seg = this.segments[i];
      if (!seg.startsWith(":") && seg !== pathSegments[i]) return null;
    }
    const params = new Map();
    for (let i = 0; i < this.segments.length; i++) {
      const seg = this.segments[i];
      if (seg.startsWith(":")) params.set(seg.slice(1), pathSegments[i]);
    }
    return params;
  }
}

class ReqContext {
  constructor(method, url, headers) {
    this.method = method;
    this.url = url;
    this.headers = headers;
    this.params = new Map();
    this.query = new Map();
    this.user = null;
    const qIdx = url.indexOf("?");
    if (qIdx !== -1) {
      this.path = url.slice(0, qIdx);
      const pairs = url.slice(qIdx + 1).split("&");
      for (const p of pairs) {
        const eq = p.indexOf("=");
        if (eq > 0) this.query.set(p.slice(0, eq), p.slice(eq + 1));
        else this.query.set(p, "1");
      }
    } else {
      this.path = url;
    }
  }
}

class ResContext {
  constructor(status, body, headers) {
    this.status = status;
    this.body = body;
    this.headers = headers;
  }
}

class Router {
  constructor() {
    this.routes = [];
  }
  add(method, pattern, handler) {
    this.routes.push(new Route(method, pattern, handler));
  }
  dispatch(req) {
    const auth = req.headers.get("authorization");
    if (auth == null || !auth.startsWith("Bearer ")) {
      return new ResContext(401, JSON.stringify({ error: "Unauthorized" }), new Map([["Content-Type", "application/json"]]));
    }
    req.user = "user_" + auth.slice(7, 12);
    const segs = [];
    for (const s of req.path.split("/")) {
      if (s.length > 0) segs.push(s);
    }
    for (let i = 0; i < this.routes.length; i++) {
      const r = this.routes[i];
      const matched = r.match(req.method, segs);
      if (matched != null) {
        req.params = matched;
        return r.handler(req);
      }
    }
    return new ResContext(404, JSON.stringify({ error: "Not Found" }), new Map([["Content-Type", "application/json"]]));
  }
}

const router = new Router();
function jsonRes(status, data) {
  return new ResContext(status, JSON.stringify(data), new Map([
    ["Content-Type", "application/json"],
    ["X-Powered-By", "Varn-Native-Router"],
  ]));
}

router.add("GET", "/api/v1/health", (req) => jsonRes(200, { status: "OK", uptime: 3600 }));
router.add("GET", "/api/v1/users", (req) => jsonRes(200, { users: ["Alice", "Bob"], page: req.query.get("page") }));
router.add("POST", "/api/v1/users", (req) => jsonRes(201, { created: true, id: 999 }));
router.add("GET", "/api/v1/users/:id", (req) => jsonRes(200, { id: req.params.get("id"), name: "User_" + req.params.get("id") }));
router.add("PUT", "/api/v1/users/:id", (req) => jsonRes(200, { updated: true, id: req.params.get("id") }));
router.add("DELETE", "/api/v1/users/:id", (req) => jsonRes(204, { deleted: true }));
router.add("GET", "/api/v1/users/:id/orders", (req) => jsonRes(200, { userId: req.params.get("id"), count: 5 }));
router.add("POST", "/api/v1/users/:id/orders", (req) => jsonRes(201, { orderId: 888, userId: req.params.get("id") }));
router.add("GET", "/api/v1/users/:id/orders/:orderId", (req) => jsonRes(200, { userId: req.params.get("id"), orderId: req.params.get("orderId"), status: "shipped" }));
router.add("GET", "/api/v1/products", (req) => jsonRes(200, { products: [1, 2, 3] }));
router.add("GET", "/api/v1/products/:id", (req) => jsonRes(200, { productId: req.params.get("id"), inStock: true }));
router.add("POST", "/api/v1/products", (req) => jsonRes(201, { created: true }));
router.add("GET", "/api/v1/categories", (req) => jsonRes(200, { categories: ["A", "B"] }));
router.add("GET", "/api/v1/categories/:id/products", (req) => jsonRes(200, { categoryId: req.params.get("id"), items: 10 }));
router.add("GET", "/api/v1/cart/:userId", (req) => jsonRes(200, { cart: req.params.get("userId"), items: 2 }));
router.add("POST", "/api/v1/cart/:userId/items", (req) => jsonRes(201, { added: true }));
router.add("DELETE", "/api/v1/cart/:userId/items/:itemId", (req) => jsonRes(200, { removed: req.params.get("itemId") }));
router.add("POST", "/api/v1/checkout/:userId", (req) => jsonRes(200, { invoice: 12345, user: req.params.get("userId") }));
router.add("GET", "/api/v1/analytics/overview", (req) => jsonRes(200, { activeUsers: 1500, sales: 45000 }));
router.add("GET", "/api/v1/system/metrics", (req) => jsonRes(200, { cpu: 12.5, memoryMb: 64 }));

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
  "/api/v1/system/metrics",
];
const methods = ["GET", "GET", "GET", "GET", "GET", "GET", "GET", "DELETE", "GET", "GET"];

const t0 = performance.now();
const reqs = [];
for (let i = 0; i < 100000; i++) {
  const idx = i % 10;
  const hdrs = new Map([
    ["authorization", `Bearer token_secret_${i % 1000}`],
    ["content-type", "application/json"],
    ["user-agent", "VarnBench/1.0"],
  ]);
  reqs.push(new ReqContext(methods[idx], samplePaths[idx], hdrs));
}
let successCount = 0;
let totalBytes = 0;
for (let i = 0; i < reqs.length; i++) {
  const res = router.dispatch(reqs[i]);
  if (res.status === 200 || res.status === 201) successCount++;
  totalBytes += res.body.length;
}
const ms = performance.now() - t0;
console.error(`HTTP Routing: Processed ${reqs.length} reqs, Success = ${successCount}, Total Response Bytes = ${totalBytes}`);
console.log(ms.toFixed(3));
