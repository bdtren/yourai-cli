(() => {
  const log = document.getElementById("log");
  const form = document.getElementById("form");
  const input = document.getElementById("input");
  const send = document.getElementById("send");
  const messages = [];

  function bubble(role, text) {
    const el = document.createElement("div");
    el.className = `msg ${role}`;
    el.textContent = text;
    log.appendChild(el);
    log.scrollTop = log.scrollHeight;
    return el;
  }

  function apiKey() {
    const q = new URLSearchParams(location.search).get("api_key");
    if (q) return q;
    try {
      return localStorage.getItem("yourai_api_key") || "";
    } catch {
      return "";
    }
  }

  async function chat(userText) {
    messages.push({ role: "user", content: userText });
    bubble("user", userText);
    const assistant = bubble("assistant", "");
    send.disabled = true;

    const headers = { "Content-Type": "application/json" };
    const key = apiKey();
    if (key) headers.Authorization = `Bearer ${key}`;

    try {
      const res = await fetch("/v1/chat/completions", {
        method: "POST",
        headers,
        body: JSON.stringify({
          model: "local",
          stream: true,
          messages,
        }),
      });
      if (!res.ok) {
        const err = await res.text();
        throw new Error(err || res.statusText);
      }

      const reader = res.body.getReader();
      const dec = new TextDecoder();
      let buf = "";
      let full = "";
      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        buf += dec.decode(value, { stream: true });
        const parts = buf.split("\n");
        buf = parts.pop() || "";
        for (const line of parts) {
          const s = line.trim();
          if (!s.startsWith("data:")) continue;
          const data = s.slice(5).trim();
          if (data === "[DONE]") continue;
          try {
            const json = JSON.parse(data);
            const delta = json.choices?.[0]?.delta?.content || "";
            if (delta) {
              full += delta;
              assistant.textContent = full;
              log.scrollTop = log.scrollHeight;
            }
          } catch {
            /* ignore partial JSON */
          }
        }
      }
      messages.push({ role: "assistant", content: full || assistant.textContent });
    } catch (e) {
      assistant.className = "msg error";
      assistant.textContent = String(e.message || e);
      messages.pop();
    } finally {
      send.disabled = false;
      input.focus();
    }
  }

  form.addEventListener("submit", (ev) => {
    ev.preventDefault();
    const text = input.value.trim();
    if (!text || send.disabled) return;
    input.value = "";
    chat(text);
  });

  input.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter" && !ev.shiftKey) {
      ev.preventDefault();
      form.requestSubmit();
    }
  });

  input.focus();
})();
