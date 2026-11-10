#!/usr/bin/env node
/**
 * Sample streaming handler for api/routes.toml (`stream = true`).
 *
 * Protocol:
 *   stdin:  { method, path, headers, query, body_b64 }  (same as buffered handlers)
 *   stdout: raw response body bytes (no JSON wrapper)
 *
 * Default Content-Type is text/event-stream unless content_type is set on the route.
 */
const chunks = [];
process.stdin.on("data", (c) => chunks.push(c));
process.stdin.on("end", () => {
  const req = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  const raw = Buffer.from(req.body_b64 || "", "base64").toString("utf8");
  const write = (s) => process.stdout.write(s);
  write("data: " + JSON.stringify({ event: "start", path: req.path }) + "\n\n");
  write("data: " + JSON.stringify({ event: "echo", text: raw }) + "\n\n");
  write("data: [DONE]\n\n");
});
