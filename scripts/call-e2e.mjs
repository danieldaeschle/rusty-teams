// Call end-to-end test with Daniel's real account and the deployed Windows app, no second person needed.
// Teams web (own Chrome, CDP 9222) joins a fresh Meet now with a looped speech file as its mic.
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
const WEB_JOIN_STEPS = ['Continue on this browser', 'Join now'];
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
  return execFileSync('cargo', ['run', '-q', '-p', 'calling', '--example', 'create_test_meeting'], { cwd: REPO_DIR, stdio: ['ignore', 'pipe', 'inherit'] })
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

function serveSpeech() {
  const speech = readFileSync(PROBE_DIR + '/web.wav');
  const server = createServer((_, response) => {
    response.writeHead(200, { 'content-type': 'audio/wav', 'access-control-allow-origin': '*', 'access-control-allow-private-network': 'true' }).end(speech);
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

class WebTab {
  constructor(browser) { this.browser = browser; }
  async open(link) {
    const { targetId } = await this.browser.send('Target.createTarget', { url: 'about:blank' });
    this.targetId = targetId;
    const { sessionId } = await this.browser.send('Target.attachToTarget', { targetId, flatten: true });
    this.sessionId = sessionId;
    await this.browser.send('Page.enable', {}, sessionId);
    await this.browser.send('Runtime.enable', {}, sessionId);
    await this.browser.send('Page.addScriptToEvaluateOnNewDocument', { source: PEER_HOOK }, sessionId);
    await this.browser.send('Page.addScriptToEvaluateOnNewDocument', { source: FAKE_MEDIA(`http://127.0.0.1:${PORT}/speech.wav`) }, sessionId);
    await this.browser.send('Browser.grantPermissions', { permissions: ['audioCapture', 'videoCapture'], origin: 'https://teams.cloud.microsoft' }).catch(() => {});
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
      const audio = await this.evaluate(INBOUND_AUDIO).catch(() => null);
      if (audio && audio.packets > 0) return true;
      for (const step of WEB_JOIN_STEPS) {
        const click = await this.evaluate(CLICK(step)).catch(() => null);
        if (click?.clicked) log(`web: clicked "${step}"`);
      }
      await delay(2000);
    }
    return false;
  }
  async shot(path) {
    const { data } = await this.browser.send('Page.captureScreenshot', { format: 'png' }, this.sessionId);
    writeFileSync(path, Buffer.from(data, 'base64'));
  }
  async close() {
    if (this.targetId) await this.browser.send('Target.closeTarget', { targetId: this.targetId }).catch(() => {});
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
const tab = new WebTab(browser);
try {
  if (!teamsProcessId()) {
    log(`starting the app, waiting ${APP_START_WAIT_MS / 1000}s`);
    runTeams();
    await delay(APP_START_WAIT_MS);
  }
  const link = givenLink ?? createMeeting();
  report.link = link;
  log('meeting ready');
  await tab.open(link);
  report.webJoined = await tab.join();
  log(`web joined: ${report.webJoined}`);
  if (!report.webJoined) {
    await tab.shot(workDir + '/web-join.png');
    throw new Error('Teams web did not join, see web-join.png');
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
  restoreCapture();
  log('default mic restored');
  server.close();
  browser.socket.close();
}
report.pass = Boolean(report.appToWeb?.pass && report.webToApp?.pass);
writeFileSync(workDir + '/report.json', JSON.stringify(report, null, 2));
console.log(JSON.stringify({ pass: report.pass, appToWeb: report.appToWeb, webToApp: report.webToApp, error: report.error }));
process.exit(report.pass ? 0 : 1);
