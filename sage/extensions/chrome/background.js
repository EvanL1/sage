const SAGE_API = "http://127.0.0.1:18522";

async function callApi(path, method, body) {
  const options = { method, headers: { "Content-Type": "application/json" } };
  if (body !== undefined) {
    options.body = JSON.stringify(body);
  }
  const res = await fetch(`${SAGE_API}${path}`, options);
  if (!res.ok) {
    throw new Error(`Sage API ${method} ${path} returned ${res.status}`);
  }
  return res.json();
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  const { type, payload } = message;

  if (type === "IMPORT_MEMORIES") {
    callApi("/api/memories", "POST", payload)
      .then((data) => sendResponse({ ok: true, data }))
      .catch((err) => sendResponse({ ok: false, error: err.message }));
    return true;
  }

  if (type === "BEHAVIOR_EVENT") {
    callApi("/api/behaviors", "POST", payload)
      .then((data) => sendResponse({ ok: true, data }))
      .catch((err) => sendResponse({ ok: false, error: err.message }));
    return true;
  }

  if (type === "CHECK_STATUS") {
    callApi("/api/status", "GET")
      .then((data) => sendResponse({ ok: true, data }))
      .catch((err) => sendResponse({ ok: false, error: err.message }));
    return true;
  }

  // Unknown message type — return false so the port closes immediately
  return false;
});
