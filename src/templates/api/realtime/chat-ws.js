// Node 22+: node --experimental-websocket chat-ws.js
// Or use any WebSocket client against ws://127.0.0.1:8080/v1/chat/ws

const url = process.env.YOURAI_WS || "ws://127.0.0.1:8080/v1/chat/ws";
const ws = new WebSocket(url);

ws.addEventListener("open", () => {
  ws.send(
    JSON.stringify({
      model: "local",
      stream: true,
      messages: [{ role: "user", content: "Say hi in five words." }],
    }),
  );
});

ws.addEventListener("message", (ev) => {
  const data = typeof ev.data === "string" ? ev.data : "";
  if (data === "[DONE]") {
    console.log("\n[DONE]");
    ws.close();
    return;
  }
  try {
    const json = JSON.parse(data);
    const delta = json.choices?.[0]?.delta?.content;
    if (delta) process.stdout.write(delta);
    else console.log(data);
  } catch {
    console.log(data);
  }
});
