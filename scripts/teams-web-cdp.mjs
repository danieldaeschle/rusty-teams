// Teams web over the Chrome DevTools Protocol: peer connection hook, fake media, stats and click helpers.

export const PEER_HOOK = `(() => {
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
export const FAKE_MEDIA = (speechUrl) => `(() => {
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

export const PEER_STATS = `(async () => {
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

export const CLICK = (text) => `(() => {
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

export class Browser {
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
