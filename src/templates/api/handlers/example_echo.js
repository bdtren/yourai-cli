#!/usr/bin/env node
/**
 * Sample custom route handler for api/routes.toml.
 *
 * Protocol (buffered JSON, non-streaming):
 *   stdin:  { method, path, headers, query, body_b64 }
 *   stdout: { status, headers, body_b64 }  (last non-empty line)
 *
 * Prefer an executable script (any language). `.js` files run with `node` when not executable.
 */
const fs = require("fs");
const input = fs.readFileSync(0, "utf8");
const req = JSON.parse(input);
const body = Buffer.from(req.body_b64 || "", "base64").toString("utf8");
const out = {
  status: 200,
  headers: { "content-type": "application/json" },
  body_b64: Buffer.from(
    JSON.stringify({ echo: body, path: req.path, method: req.method }),
    "utf8"
  ).toString("base64"),
};
process.stdout.write(JSON.stringify(out) + "\n");
