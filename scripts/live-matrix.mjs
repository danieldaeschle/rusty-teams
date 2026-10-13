// Live meeting matrix driver: runs the `live_matrix` example as organizer and drives Teams web guests over CDP.
// Usage: node scripts/live-matrix.mjs <live_matrix binary> <work dir>; then talk to http://127.0.0.1:9472 (see ROUTES).
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { readFileSync, writeFileSync, existsSync } from 'node:fs';

const [binary, workDir] = process.argv.slice(2);
const GUEST_ENDPOINT = process.env.GUEST_CDP ?? 'http://127.0.0.1:9471';
const OWN_ENDPOINT = process.env.OWN_CDP ?? 'http://127.0.0.1:9222';
const PORT = Number(process.env.MATRIX_PORT ?? 9472);

const PEER_HOOK = `(() => {
  const Original = window.RTCPeerConnection;
  if (!Original || Original.__matrix) return;
  window.__pcs = [];
  const Wrapped = function (...args) { const peer = new Original(...args); window.__pcs.push(peer); return peer; };
  Wrapped.prototype = Original.prototype;
  Object.setPrototypeOf(Wrapped, Original);
  Wrapped.__matrix = true;
  window.RTCPeerConnection = Wrapped;
})();`;

// Signed-in Chrome has no fake devices: camera = moving canvas, mic = looped speech file, screen = canvas.
const FAKE_MEDIA = (speechUrl) => `(() => {
  if (!navigator.mediaDevices || navigator.mediaDevices.__matrix) return;
  const canvasTrack = (label, width, height) => {
    const canvas = document.createElement('canvas'); canvas.width = width; canvas.height = height;
    const context = canvas.getContext('2d'); let frame = 0;
    setInterval(() => { frame++; context.fillStyle = 'hsl(' + (frame * 3 % 360) + ',70%,40%)'; context.fillRect(0, 0, width, height);
      context.fillStyle = '#fff'; context.font = '48px sans-serif'; context.fillText(label + ' ' + frame, 40, height / 2); }, 66);
    return canvas.captureStream(15).getVideoTracks()[0];
  };
  let audioContext;
  const speechTrack = async () => {
    audioContext = audioContext || new AudioContext();
    const destination = audioContext.createMediaStreamDestination();
    try {
      const data = await (await fetch('${speechUrl}')).arrayBuffer();
      const buffer = await audioContext.decodeAudioData(data);
      const source = audioContext.createBufferSource(); source.buffer = buffer; source.loop = true; source.connect(destination); source.start();
    } catch (error) {
      const oscillator = audioContext.createOscillator(); oscillator.frequency.value = 440; oscillator.connect(destination); oscillator.start();
    }
    return destination.stream.getAudioTracks()[0];
  };
  const devices = [
    { deviceId: 'matrix-mic', kind: 'audioinput', label: 'Matrix mic', groupId: 'matrix' },
    { deviceId: 'matrix-cam', kind: 'videoinput', label: 'Matrix camera', groupId: 'matrix' },
    { deviceId: 'matrix-speaker', kind: 'audiooutput', label: 'Matrix speaker', groupId: 'matrix' },
  ].map((device) => ({ ...device, toJSON() { return device; } }));
  navigator.mediaDevices.enumerateDevices = async () => devices;
  navigator.mediaDevices.getUserMedia = async (constraints) => {
    const tracks = [];
    if (constraints && constraints.audio) tracks.push(await speechTrack());
    if (constraints && constraints.video) tracks.push(canvasTrack('camera', 640, 360));
    return new MediaStream(tracks);
  };
  navigator.mediaDevices.getDisplayMedia = async () => new MediaStream([canvasTrack('screen', 1280, 720)]);
  navigator.mediaDevices.__matrix = true;
})();`;

const PEER_STATS = `(async () => {
  const report = { peers: 0, audioIn: [], videoIn: [], audioOut: [], videoOut: [] };
  for (const peer of window.__pcs || []) {
    if (peer.connectionState === 'closed') continue;
    report.peers++;
    (await peer.getStats()).forEach((entry) => {
      if (entry.type === 'inbound-rtp' && entry.kind === 'audio') report.audioIn.push({ energy: entry.totalAudioEnergy, level: entry.audioLevel, packets: entry.packetsReceived });
      if (entry.type === 'inbound-rtp' && entry.kind === 'video') report.videoIn.push({ mid: entry.mid, decoded: entry.framesDecoded || 0, width: entry.frameWidth, height: entry.frameHeight });
      if (entry.type === 'outbound-rtp') report[entry.kind + 'Out'].push({ mid: entry.mid, packets: entry.packetsSent, frames: entry.framesEncoded, width: entry.frameWidth, height: entry.frameHeight });
    });
  }
  return report;
})()`;

class Browser {
  constructor(endpoint) { this.endpoint = endpoint; this.nextId = 1; this.pending = new Map(); this.listeners = []; }
  async connect() {
    const version = await (await fetch(this.endpoint + '/json/version')).json();
    this.socket = new WebSocket(version.webSocketDebuggerUrl);
    await new Promise((resolve, reject) => { this.socket.onopen = resolve; this.socket.onerror = reject; });
    this.socket.onmessage = (message) => {
      const data = JSON.parse(message.data);
      if (data.id && this.pending.has(data.id)) {
        const { resolve, reject } = this.pending.get(data.id); this.pending.delete(data.id);
        data.error ? reject(new Error(data.error.message)) : resolve(data.result);
      } else if (data.method) this.listeners.forEach((listener) => listener(data));
    };
    return this;
  }
  send(method, params = {}, sessionId) {
    const id = this.nextId++;
    this.socket.send(JSON.stringify({ id, method, params, sessionId }));
    return new Promise((resolve, reject) => { this.pending.set(id, { resolve, reject }); setTimeout(() => { if (this.pending.delete(id)) reject(new Error(method + ' timed out')); }, 60000); });
  }
}

const guests = new Map();
const events = [];
let child;
let guestBrowser;
let ownBrowser;

// Shared Chrome: other clients reload or close the first Teams tab, which took our Trouter with it.
const TAB_PROXY_PORT = PORT + 1;
let organizerTabId;

async function openOrganizerTab() {
  if (!organizerTabId) {
    organizerTabId = (await ownBrowser.send('Target.createTarget', { url: 'https://teams.cloud.microsoft/robots.txt' })).targetId;
    const others = (await (await fetch(OWN_ENDPOINT + '/json/list')).json()).filter((target) => target.type === 'page' && target.id !== organizerTabId && target.url.startsWith('https://teams.cloud.microsoft'));
    if (others[0]) await ownBrowser.send('Target.activateTarget', { targetId: others[0].id }).catch(() => {});
  }
  return organizerTabId;
}

createServer(async (request, response) => {
  const upstream = await fetch(OWN_ENDPOINT + request.url);
  let body = await upstream.text();
  if (request.url.startsWith('/json/list') || request.url === '/json') {
    const targets = JSON.parse(body);
    body = JSON.stringify(targets.filter((target) => target.id === organizerTabId || !target.url.startsWith('https://teams.')));
  }
  response.writeHead(upstream.status, { 'content-type': 'application/json', 'content-length': Buffer.byteLength(body) }).end(body);
}).listen(TAB_PROXY_PORT, '127.0.0.1');

async function startOrganizer() {
  await openOrganizerTab();
  child = spawn(binary, ['--link-out', workDir + '/link.txt', '--thread-out', workDir + '/thread.txt'], {
    stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env, CDP_ENDPOINT: `http://127.0.0.1:${TAB_PROXY_PORT}` },
  });
  let buffer = '';
  child.stdout.on('data', (chunk) => {
    buffer += chunk;
    let index;
    while ((index = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0, index); buffer = buffer.slice(index + 1);
      if (line.startsWith('@')) { try { events.push({ at: Date.now(), ...JSON.parse(line.slice(1)) }); } catch { /* partial line */ } }
    }
  });
  child.stderr.on('data', (chunk) => writeFileSync(workDir + '/organizer.stderr', chunk, { flag: 'a' }));
  child.on('exit', (code) => {
    events.push({ at: Date.now(), event: 'organizer_exit', code });
    ownBrowser.send('Target.closeTarget', { targetId: organizerTabId }).catch(() => {});
    organizerTabId = undefined;
  });
  child.stdout.on('data', (chunk) => writeFileSync(workDir + '/organizer.stdout', chunk, { flag: 'a' }));
}

async function evaluate(guest, expression) {
  const result = await guest.browser.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }, guest.sessionId);
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? 'evaluate failed');
  return result.result.value;
}

const CLICK = (text) => `(() => {
  const wanted = ${JSON.stringify(text)}.toLowerCase();
  const nodes = [...document.querySelectorAll('button, [role=button], [role=menuitem], [role=menuitemcheckbox], [role=tab], a, [role=option]')];
  const label = (node) => ((node.getAttribute('aria-label') || '') + ' ' + (node.innerText || '') + ' ' + (node.getAttribute('title') || '')).trim().toLowerCase();
  const visible = nodes.filter((node) => node.offsetParent !== null || node.getClientRects().length);
  const exact = visible.find((node) => (node.innerText || '').trim().toLowerCase() === wanted || (node.getAttribute('aria-label') || '').trim().toLowerCase() === wanted);
  const target = exact || visible.find((node) => label(node).includes(wanted));
  if (!target) return { clicked: false, candidates: visible.map(label).filter(Boolean).slice(0, 60) };
  target.scrollIntoView({ block: 'center' }); target.click();
  return { clicked: true, label: label(target).slice(0, 80) };
})()`;

async function openGuest({ name, own = false, url }) {
  const browser = own ? ownBrowser : guestBrowser;
  let browserContextId;
  if (!own) browserContextId = (await browser.send('Target.createBrowserContext', { disposeOnDetach: false })).browserContextId;
  const { targetId } = await browser.send('Target.createTarget', { url: 'about:blank', ...(browserContextId ? { browserContextId } : {}) });
  const { sessionId } = await browser.send('Target.attachToTarget', { targetId, flatten: true });
  const guest = { name, own, browser, browserContextId, targetId, sessionId };
  await browser.send('Page.enable', {}, sessionId);
  await browser.send('Runtime.enable', {}, sessionId);
  await browser.send('Page.addScriptToEvaluateOnNewDocument', { source: PEER_HOOK }, sessionId);
  if (own) {
    await browser.send('Page.addScriptToEvaluateOnNewDocument', { source: FAKE_MEDIA(`http://127.0.0.1:${PORT}/speech.wav`) }, sessionId);
    await browser.send('Browser.grantPermissions', { permissions: ['audioCapture', 'videoCapture', 'displayCapture'], origin: 'https://teams.cloud.microsoft' }).catch(() => {});
  }
  await browser.send('Emulation.setFocusEmulationEnabled', { enabled: true }, sessionId).catch(() => {});
  await browser.send('Page.navigate', { url: url ?? readFileSync(workDir + '/link.txt', 'utf8').trim() }, sessionId);
  guests.set(name, guest);
  return { opened: name };
}

async function closeGuest(name) {
  const guest = guests.get(name);
  if (!guest) return { closed: false };
  await guest.browser.send('Target.closeTarget', { targetId: guest.targetId }).catch(() => {});
  if (guest.browserContextId) await guest.browser.send('Target.disposeBrowserContext', { browserContextId: guest.browserContextId }).catch(() => {});
  guests.delete(name);
  return { closed: true };
}

async function screenshot(name, path) {
  const guest = guests.get(name);
  const { data } = await guest.browser.send('Page.captureScreenshot', { format: 'png' }, guest.sessionId);
  writeFileSync(path, Buffer.from(data, 'base64'));
  return { path };
}

async function typeText(name, selector, text) {
  const guest = guests.get(name);
  const focused = await evaluate(guest, `(() => { const node = document.querySelector(${JSON.stringify(selector)}); if (!node) return false; node.focus(); node.select && node.select(); return true; })()`);
  if (focused) await guest.browser.send('Input.insertText', { text }, guest.sessionId);
  return { typed: focused };
}

async function pressKey(name, key, modifiers = 0) {
  const guest = guests.get(name);
  const codes = { Enter: 13, Escape: 27, Space: 32 };
  for (const type of ['keyDown', 'keyUp']) {
    await guest.browser.send('Input.dispatchKeyEvent', { type, key, code: key, windowsVirtualKeyCode: codes[key] ?? key.toUpperCase().charCodeAt(0), modifiers }, guest.sessionId);
  }
  return { pressed: key };
}

async function capture(name) {
  const guest = guests.get(name);
  guest.requests = [];
  guest.browser.listeners.push((message) => {
    if (message.sessionId === guest.sessionId && message.method === 'Network.requestWillBeSent') {
      const { url, method, postData } = message.params.request;
      guest.requests.push({ url, method, postData });
    }
  });
  await guest.browser.send('Network.enable', {}, guest.sessionId);
  return { capturing: name };
}

const trouterFrames = [];
async function watchTrouter() {
  const { sessionId } = await ownBrowser.send('Target.attachToTarget', { targetId: organizerTabId, flatten: true });
  ownBrowser.listeners.push((message) => {
    if (message.sessionId !== sessionId || message.method !== 'Network.webSocketFrameReceived') return;
    const payload = message.params.response.payloadData || '';
    const path = /callAgent\/[^"]*?\/(call|conversation)\/([A-Za-z]+)/.exec(payload);
    if (!path) return;
    const sequence = /sequenceNumber\\?":(\d+)/.exec(payload);
    trouterFrames.push({ at: Date.now(), event: path[2], url: /\\?"url\\?":\\?"([^"\\]{0,60})/.exec(payload)?.[1], head: payload.slice(0, 12), sequence: sequence ? Number(sequence[1]) : null, bytes: payload.length });
  });
  await ownBrowser.send('Network.enable', {}, sessionId);
  return { watching: true };
}

const ROUTES = {
  'POST /organizer/watch-trouter': async () => watchTrouter(),
  'GET /organizer/trouter-frames': async () => trouterFrames,
  'POST /guest/capture': async (body) => capture(body.name),
  'POST /guest/requests': async (body) => (guests.get(body.name).requests ?? []).filter((request) => request.url.includes(body.match ?? '')),
  'POST /organizer/start': async () => { await startOrganizer(); return { started: true }; },
  'POST /organizer/send': async (body) => { child.stdin.write(body.line + '\n'); return { sent: body.line }; },
  'GET /events': async (_, query) => events.filter((event) => event.at > Number(query.get('since') ?? 0) && (!query.get('type') || event.event === query.get('type'))),
  'POST /guest/open': async (body) => openGuest(body),
  'POST /guest/close': async (body) => closeGuest(body.name),
  'POST /guest/eval': async (body) => evaluate(guests.get(body.name), body.expression),
  'POST /guest/click': async (body) => evaluate(guests.get(body.name), CLICK(body.text)),
  'POST /guest/type': async (body) => typeText(body.name, body.selector, body.text),
  'POST /guest/key': async (body) => pressKey(body.name, body.key, body.modifiers),
  'POST /guest/shot': async (body) => screenshot(body.name, body.path),
  'POST /guest/stats': async (body) => evaluate(guests.get(body.name), PEER_STATS),
  'POST /guest/text': async (body) => evaluate(guests.get(body.name), `document.body.innerText.slice(0, ${body.limit ?? 3000})`),
  'GET /guests': async () => [...guests.keys()],
};

guestBrowser = await new Browser(GUEST_ENDPOINT).connect();
ownBrowser = await new Browser(OWN_ENDPOINT).connect();
if (!existsSync(workDir)) throw new Error('work dir missing');

createServer(async (request, response) => {
  const url = new URL(request.url, 'http://localhost');
  if (url.pathname === '/speech.wav') {
    response.writeHead(200, { 'content-type': 'audio/wav', 'access-control-allow-origin': '*', 'access-control-allow-private-network': 'true' }).end(readFileSync(workDir + '/speech.wav'));
    return;
  }
  const route = ROUTES[request.method + ' ' + url.pathname];
  let raw = '';
  for await (const chunk of request) raw += chunk;
  try {
    if (!route) throw new Error('no route');
    const result = await route(raw ? JSON.parse(raw) : {}, url.searchParams);
    response.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(result ?? null));
  } catch (error) {
    response.writeHead(500, { 'content-type': 'application/json' }).end(JSON.stringify({ error: String(error.message ?? error) }));
  }
}).listen(PORT, '127.0.0.1');
console.log('live matrix driver on ' + PORT);
