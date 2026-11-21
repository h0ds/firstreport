// node deploy/worker.test.mjs
import assert from "node:assert/strict";
import worker, { archiveKey } from "./worker.js";

assert.equal(archiveKey("/story/12/"), "story/12/index.html");
assert.equal(archiveKey("/story/12/index.md"), "story/12/index.md");
for (const p of ["/", "/story/", "/story/x/", "/story/12/../../secret", "/outlet/bbc/", "/story/12/evil.js"]) {
  assert.equal(archiveKey(p), null, p);
}

const asked = [];
const env = {
  ARCHIVE: {
    async get(key) {
      asked.push(key);
      return key === "story/12/index.html" ? { body: "<h1>old</h1>", httpEtag: '"abc"' } : null;
    },
  },
  ASSETS: { fetch: async () => new Response("not found", { status: 404 }) },
};
const get = (path, method = "GET") => worker.fetch(new Request("https://x.example" + path, { method }), env);

let r = await get("/story/12/");
assert.equal(r.status, 200);
assert.equal(await r.text(), "<h1>old</h1>");
assert.match(r.headers.get("Content-Type"), /^text\/html/);
assert.equal(r.headers.get("ETag"), '"abc"');

r = await get("/story/12", "GET");
assert.equal(r.status, 308);
assert.equal(r.headers.get("Location"), "https://x.example/story/12/");

r = await get("/story/99/");
assert.equal(r.status, 404);

asked.length = 0;
r = await get("/outlet/bbc/");
assert.equal(r.status, 404);
assert.deepEqual(asked, [], "only story paths reach the bucket");

r = await get("/story/12/", "POST");
assert.equal(r.status, 404);
console.log("worker ok");
