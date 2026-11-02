// Optional private-source oracle. Public tests consume only frozen synthetic fields.
const fs = require("node:fs"),
  path = require("node:path"),
  vm = require("node:vm"),
  assert = require("node:assert/strict"),
  crypto = require("node:crypto");
const parser = require("../../frontend/node_modules/@babel/parser");
const root = path.resolve(
  process.argv[2] ||
    "references/netease-3.1.42.205594-analysis/recovered-js/modules",
);
const names = [
  "hybrid_vendors_app_subApp.chunk.1482700000000.js/1368.js",
  "hybrid_app.chunk.1482700000000.js/15.js",
  "hybrid_app.chunk.1482700000000.js/48.js",
];
const src = names.map((n) => fs.readFileSync(path.join(root, n), "utf8")),
  ast = src.map((s) => parser.parse(s));
function walk(n, visit) {
  if (!n || typeof n !== "object") return;
  visit(n);
  for (const v of Object.values(n))
    Array.isArray(v) ? v.forEach((a) => walk(a, visit)) : walk(v, visit);
}
const code = (n, i = 0) => src[i].slice(n.start, n.end);
let serializer;
walk(ast[2], (n) => {
  if (n.type === "FunctionDeclaration" && n.id.name === "f") serializer = n;
});
assert(serializer);
const form = vm.runInNewContext("(" + code(serializer, 2) + ")", {
  i: { a: (v) => v !== null && typeof v === "object" && !Array.isArray(v) },
  s: encodeURIComponent,
  u: (v) => "[" + v.map(encodeURIComponent).join(",") + "]",
  d: (v) => encodeURIComponent(JSON.stringify(v)),
});
const args = {
  sendDownSms: { e: undefined },
  verifyDownSms: { e: undefined, n: "123456", r: undefined, i: undefined },
  getUpSmsInfo: {
    e: "138****8000",
    n: undefined,
    r: "music_test",
    i: undefined,
  },
  verifyUpSms: {
    e: "138****8000",
    n: "SEND 1234",
    r: 30000,
    i: undefined,
    o: "music_test",
    a: undefined,
  },
};
const jsk = {};
for (const [name, env] of Object.entries(args)) {
  const found = [];
  walk(ast[0], (n) => {
    if (
      n.type === "ObjectExpression" &&
      n.properties.some((p) => p.key?.name === "key" && p.value?.value === name)
    ) {
      const fn = n.properties.find((p) => p.key?.name === "value")?.value;
      if (fn && code(fn).includes(".fetch(")) found.push(fn);
    }
  });
  assert.equal(found.length, 1, name);
  let call;
  walk(found[0], (n) => {
    if (n.type === "CallExpression" && n.callee.property?.name === "fetch")
      call = n;
  });
  assert(call);
  const value = vm.runInNewContext("(" + code(call.arguments[1]) + ")", {
    ...env,
    U: Object.assign,
  });
  value.fromPage = "JSK";
  jsk[name] = {
    path: call.arguments[0].value,
    payload: Object.fromEntries(new URLSearchParams(form(value))),
  };
}
const legacy = {};
walk(ast[1], (n) => {
  if (n.type !== "CallExpression" || n.callee.type !== "CallExpression") return;
  const config = n.callee.arguments[0];
  if (config?.type !== "ObjectExpression") return;
  const url = config.properties.find((p) => p.key?.name === "url")?.value
    ?.value;
  if (!url?.startsWith("/api/middle/account/")) return;
  legacy[url] = vm.runInNewContext("(" + code(n.arguments[0], 1) + ")", {
    e: url.includes("uplink") ? "SEND 1234" : "123456",
  });
});
// M15 Wa is a request helper invoked with no arguments by DoubleVerifySmsCaptcha.
legacy["/api/middle/account/captcha/sent"] = {};
assert.equal(Object.keys(legacy).length, 4);
let stopped = 0;
walk(ast[0], (n) => {
  if (
    n.type !== "ObjectExpression" ||
    !n.properties.some(
      (p) => p.key?.name === "key" && p.value?.value === "stopUpSmsPolling",
    )
  )
    return;
  const fn = n.properties.find((p) => p.key?.name === "value")?.value;
  if (!fn || !code(fn).includes("shouldStopPolling")) return;
  const receiver = { shouldStopPolling: false };
  vm.runInNewContext("(" + code(fn) + ")", { Ve: { log() {} }, Je() {} }).call(
    receiver,
  );
  assert.equal(receiver.shouldStopPolling, true);
  stopped++;
});
assert.equal(stopped, 1);
console.log(
  JSON.stringify(
    {
      sources: Object.fromEntries(
        names.map((n, i) => [
          n,
          crypto.createHash("sha256").update(src[i]).digest("hex"),
        ]),
      ),
      jsk,
      legacy,
      stopBehavior: { localOnly: true, networkRequests: 0 },
    },
    null,
    2,
  ),
);
