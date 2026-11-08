// Call end-to-end test with Daniel's real account and the deployed Windows app, no second person needed.
// Teams web as Daniel hosts a fresh Meet now and admits an anonymous Teams web guest whose mic is a looped speech file.
// The Windows app joins through `teams.exe --join`; its mic is VB-CABLE fed with a second speech file,
// and process loopback records what the app plays. Both directions must show speech with pauses.
// Usage: node scripts/call-e2e.mjs <work dir> [meeting link]
import { spawn, execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { readFileSync, writeFileSync } from 'node:fs';
import { Browser, CLICK, FAKE_MEDIA, PEER_HOOK } from './teams-web-cdp.mjs';

const [workDir, givenLink] = process.argv.slice(2);
const OWN_ENDPOINT = process.env.OWN_CDP ?? 'http://127.0.0.1:9222';
const PORT = Number(process.env.E2E_PORT ?? 9475);
const PROBE_DIR = process.env.PROBE_DIR ?? '/mnt/c/Users/dd/AppData/Local/Temp/call-e2e';
const TEAMS_EXE = process.env.TEAMS_EXE ?? '/mnt/c/Users/dd/AppData/Local/Programs/ms-teams-linux/teams.exe';
const POWERSHELL = '/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe';
const TASKLIST = '/mnt/c/Windows/System32/tasklist.exe';
const SCRIPT_DIR = new URL('.', import.meta.url).pathname;
const REPO_DIR = new URL('..', import.meta.url).pathname;
const APP_START_WAIT_MS = 40_000;
const WEB_JOIN_TIMEOUT_MS = 90_000;
const APP_JOIN_WAIT_MS = 25_000;
const MEASURE_SECONDS = 30;
const WINDOW_MS = 250;
const WEB_JOIN_STEPS = ['Continue on this browser', 'Not now', 'Join now'];
const GUEST_NAME = 'Call e2e guest';
const LOBBY_TEXTS = ['let you in shortly', 'should let you in soon'];
const WEB_THRESHOLD = { speech: 0.005, floor: 0.0005 };
const APP_THRESHOLD = { speech: 200, floor: 20 };
const SPEECH_TO_PAUSE = 4;

const windowsPath = (path) => execFileSync('wslpath', ['-w', path]).toString().trim();
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const log = (message) => console.error(`[${new Date().toISOString().slice(11, 19)}] ${message}`);

function defaultAudio(...args) {
  const script = windowsPath(SCRIPT_DIR + 'default-audio.ps1');
  return execFileSync(POWERSHELL, ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', script, ...args]).toString().trim();
}

function createMeeting() {
  return execFileSync('cargo', ['run', '-q', '-p', 'chatsvc', '--example', 'create_test_meeting'], { cwd: REPO_DIR, stdio: ['ignore', 'pipe', 'inherit'] })
    .toString()
    .trim();
}

function teamsProcessId() {
  const rows = execFileSync(TASKLIST, ['/FI', 'IMAGENAME eq teams.exe', '/FO', 'CSV', '/NH']).toString().trim().split(/\r?\n/);
  const processes = rows
    .map((row) => row.split('","').map((cell) => cell.replace(/"/g, '')))
    .filter((cells) => cells[0]?.toLowerCase() === 'teams.exe')
    .map((cells) => ({ id: Number(cells[1]), memory: Number(cells[4].replace(/\D/g, '')) }));
  processes.sort((left, right) => right.memory - left.memory);
  return processes[0]?.id;
}

function runTeams(...args) {
  spawn(TEAMS_EXE, args, { detached: true, stdio: 'ignore' }).unref();
}

function runProbe(...args) {
  return new Promise((resolve, reject) => {
    const child = spawn(PROBE_DIR + '/audio_probe.exe', args, { stdio: ['ignore', 'pipe', 'pipe'] });
    let output = '';
    let errors = '';
    child.stdout.on('data', (chunk) => (output += chunk));
    child.stderr.on('data', (chunk) => (errors += chunk));
    child.on('exit', (code) => (code === 0 ? resolve(output) : reject(new Error(`audio_probe ${args[0]}: ${errors.trim()}`))));
  });
}

function silentWav(seconds) {
  const dataBytes = seconds * 48000 * 2;
  const header = Buffer.alloc(44);
  header.write('RIFF', 0); header.writeUInt32LE(36 + dataBytes, 4); header.write('WAVEfmt ', 8);
  header.writeUInt32LE(16, 16); header.writeUInt16LE(1, 20); header.writeUInt16LE(1, 22); header.writeUInt32LE(48000, 24);
  header.writeUInt32LE(96000, 28); header.writeUInt16LE(2, 32); header.writeUInt16LE(16, 34); header.write('data', 36); header.writeUInt32LE(dataBytes, 40);
  return Buffer.concat([header, Buffer.alloc(dataBytes)]);
}

function serveSpeech() {
  const files = { '/speech.wav': readFileSync(PROBE_DIR + '/web.wav'), '/silence.wav': silentWav(1) };
  const server = createServer((request, response) => {
    response.writeHead(200, { 'content-type': 'audio/wav', 'access-control-allow-origin': '*', 'access-control-allow-private-network': 'true' }).end(files[request.url] ?? files['/silence.wav']);
  });
  server.listen(PORT, '127.0.0.1');
  return server;
}

const INBOUND_AUDIO = `(async () => {
  let energy = 0, duration = 0, packets = 0;
  for (const peer of window.__pcs || []) {
    if (peer.connectionState === 'closed') continue;
    (await peer.getStats()).forEach((entry) => {
      if (entry.type !== 'inbound-rtp' || entry.kind !== 'audio') return;
      energy += entry.totalAudioEnergy || 0; duration += entry.totalSamplesDuration || 0; packets += entry.packetsReceived || 0;
    });
  }
  return { energy, duration, packets };
})()`;

const GUEST_SENDING = `(async () => {
  let energy = 0, duration = 0, packets = 0;
  for (const peer of window.__pcs || []) {
    if (peer.connectionState === 'closed') continue;
    (await peer.getStats()).forEach((entry) => {
      if (entry.type === 'media-source' && entry.kind === 'audio') { energy += entry.totalAudioEnergy || 0; duration += entry.totalSamplesDuration || 0; }
      if (entry.type === 'outbound-rtp' && entry.kind === 'audio') packets += entry.packetsSent || 0;
    });
  }
  return { level: duration > 0 ? Math.sqrt(energy / duration) : 0, packets };
})()`;
const CONNECTED = `(window.__pcs || []).some((peer) => peer.connectionState === 'connected')`;
const TEAMS_ORIGINS = ['https://teams.cloud.microsoft', 'https://teams.microsoft.com'];

class WebTab {
  constructor(browser, { guest }) { this.browser = browser; this.guest = guest; }
  async open(link) {
    if (this.guest) this.contextId = (await this.browser.send('Target.createBrowserContext', { disposeOnDetach: false })).browserContextId;
    const context = this.contextId ? { browserContextId: this.contextId } : {};
    const { targetId } = await this.browser.send('Target.createTarget', { url: 'about:blank', ...context });
    this.targetId = targetId;
    const { sessionId } = await this.browser.send('Target.attachToTarget', { targetId, flatten: true });
    this.sessionId = sessionId;
    await this.browser.send('Page.enable', {}, sessionId);
    await this.browser.send('Runtime.enable', {}, sessionId);
    await this.browser.send('Page.addScriptToEvaluateOnNewDocument', { source: PEER_HOOK }, sessionId);
    await this.browser.send('Page.addScriptToEvaluateOnNewDocument', { source: FAKE_MEDIA(`http://127.0.0.1:${PORT}/${this.guest ? 'speech' : 'silence'}.wav`) }, sessionId);
    for (const origin of TEAMS_ORIGINS) {
      await this.browser.send('Browser.grantPermissions', { permissions: ['audioCapture', 'videoCapture'], origin, ...context }).catch(() => {});
    }
    await this.browser.send('Emulation.setFocusEmulationEnabled', { enabled: true }, sessionId).catch(() => {});
    await this.browser.send('Page.navigate', { url: link }, sessionId);
  }
  async evaluate(expression) {
    const result = await this.browser.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }, this.sessionId);
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? 'evaluate failed');
    return result.result.value;
  }
  async join() {
    const deadline = Date.now() + WEB_JOIN_TIMEOUT_MS;
    while (Date.now() < deadline) {
      const inLobby = await this.evaluate(`${JSON.stringify(LOBBY_TEXTS)}.some((text) => document.body.innerText.includes(text))`).catch(() => false);
      if (!inLobby && (await this.evaluate(CONNECTED).catch(() => false))) return true;
      if (this.guest) await this.typeName();
      for (const step of WEB_JOIN_STEPS) {
        const click = await this.evaluate(CLICK(step)).catch(() => null);
        if (click?.clicked) log(`web: clicked "${step}"`);
      }
      await delay(2000);
    }
    return false;
  }
  async typeName() {
    const focused = await this.evaluate(`(() => {
      const input = document.querySelector('input[placeholder*="name" i]');
      if (!input || input.value) return false;
      input.focus(); return true;
    })()`).catch(() => false);
    if (focused) await this.browser.send('Input.insertText', { text: GUEST_NAME }, this.sessionId);
  }
  async admitGuests() {
    const click = await this.evaluate(CLICK('Admit')).catch(() => null);
    if (click?.clicked) log(`web: clicked "${click.label}"`);
    else if (!this.peopleOpen) this.peopleOpen = (await this.evaluate(CLICK('People')).catch(() => null))?.clicked;
  }
  async shot(path) {
    const { data } = await this.browser.send('Page.captureScreenshot', { format: 'png' }, this.sessionId);
    writeFileSync(path, Buffer.from(data, 'base64'));
  }
  async close() {
    if (this.targetId) await this.browser.send('Target.closeTarget', { targetId: this.targetId }).catch(() => {});
    if (this.contextId) await this.browser.send('Target.disposeBrowserContext', { browserContextId: this.contextId }).catch(() => {});
  }
}

async function sampleWeb(tab, seconds) {
  const levels = [];
  let previous = await tab.evaluate(INBOUND_AUDIO);
  const end = Date.now() + seconds * 1000;
  while (Date.now() < end) {
    await delay(WINDOW_MS);
    const current = await tab.evaluate(INBOUND_AUDIO);
    const duration = current.duration - previous.duration;
    levels.push(duration > 0 ? Math.sqrt(Math.max(0, current.energy - previous.energy) / duration) : 0);
    previous = current;
  }
  return levels;
}

function judge(levels, threshold) {
  const sorted = [...levels].sort((left, right) => left - right);
  const percentile = (share) => sorted[Math.min(sorted.length - 1, Math.floor(share * sorted.length))] ?? 0;
  const [p10, p90] = [percentile(0.1), percentile(0.9)];
  const speech = p90 >= threshold.speech;
  const pauses = p90 >= SPEECH_TO_PAUSE * Math.max(p10, threshold.floor);
  return { pass: speech && pauses, p10, p90, windows: levels.length };
}

const report = { started: new Date().toISOString() };
const savedCapture = defaultAudio('get', 'capture');
const restoreCapture = () => defaultAudio('set', 'capture', savedCapture);
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => { restoreCapture(); process.exit(130); });
const server = serveSpeech();
const browser = await new Browser(OWN_ENDPOINT).connect();
const host = new WebTab(browser, { guest: false });
const tab = new WebTab(browser, { guest: true });
try {
  if (!teamsProcessId()) {
    log(`starting the app, waiting ${APP_START_WAIT_MS / 1000}s`);
    runTeams();
    await delay(APP_START_WAIT_MS);
  }
  const link = givenLink ?? createMeeting();
  report.link = link;
  log('meeting ready');
  await host.open(link);
  if (!(await host.join())) {
    await host.shot(workDir + '/host-join.png');
    throw new Error('Teams web host did not join, see host-join.png');
  }
  log('web host joined');
  await tab.open(link);
  const admitting = setInterval(() => host.admitGuests(), 2000);
  report.webJoined = await tab.join().finally(() => clearInterval(admitting));
  log(`web guest joined: ${report.webJoined}`);
  if (!report.webJoined) {
    await tab.shot(workDir + '/web-join.png');
    await host.shot(workDir + '/host-admit.png');
    throw new Error('Teams web guest did not join, see web-join.png and host-admit.png');
  }
  defaultAudio('set', 'capture', 'CABLE Output');
  log('default mic: CABLE Output');
  const playSeconds = Math.ceil(APP_JOIN_WAIT_MS / 1000) + MEASURE_SECONDS + 10;
  const playing = runProbe('play', 'CABLE Input', windowsPath(PROBE_DIR + '/app.wav'), String(playSeconds));
  runTeams('--join', link);
  log(`app joining, waiting ${APP_JOIN_WAIT_MS / 1000}s`);
  await delay(APP_JOIN_WAIT_MS);
  const processId = teamsProcessId();
  if (!processId) throw new Error('teams.exe is not running');
  log(`measuring ${MEASURE_SECONDS}s (teams.exe pid ${processId})`);
  const [recorded, webLevels] = await Promise.all([
    runProbe('record-process', String(processId), String(MEASURE_SECONDS), windowsPath(workDir + '/app-heard.wav')),
    sampleWeb(tab, MEASURE_SECONDS),
  ]);
  report.guestSending = await tab.evaluate(GUEST_SENDING).catch((error) => error.message);
  report.appToWeb = judge(webLevels, WEB_THRESHOLD);
  report.webToApp = judge(JSON.parse(recorded).rms, APP_THRESHOLD);
  report.webLevels = webLevels.map((level) => Number(level.toFixed(4)));
  report.appLevels = JSON.parse(recorded).rms;
  await tab.shot(workDir + '/web-in-call.png');
  await playing.catch((error) => (report.playError = error.message));
} catch (error) {
  report.error = error.message;
} finally {
  if (teamsProcessId()) runTeams('--hang-up');
  await tab.close();
  await host.close();
  restoreCapture();
  log('default mic restored');
  server.close();
  browser.socket.close();
}
report.pass = Boolean(report.appToWeb?.pass && report.webToApp?.pass);
writeFileSync(workDir + '/report.json', JSON.stringify(report, null, 2));
console.log(JSON.stringify({ pass: report.pass, appToWeb: report.appToWeb, webToApp: report.webToApp, guestSending: report.guestSending, error: report.error }));
process.exit(report.pass ? 0 : 1);
