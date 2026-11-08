// Call end-to-end test with Daniel's real account and the deployed Windows app, no second person needed.
// The Windows app hosts a fresh Meet now without lobby through `teams.exe --join`; its mic is VB-CABLE fed with a speech file.
// An anonymous Teams web guest (own incognito context) joins with a second speech file as its mic,
// and process loopback records what the app plays. Both directions must show speech with pauses.
// After audio it runs the video, screen share, mute and rejoin scenarios and judges each from both sides' stats.
// Usage: node scripts/call-e2e.mjs <work dir> [meeting link]
import { spawn, execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { Browser, FAKE_MEDIA, PEER_HOOK, PEER_STATS } from './teams-web-cdp.mjs';

const [workDir, givenLink] = process.argv.slice(2);
const OWN_ENDPOINT = process.env.OWN_CDP ?? 'http://127.0.0.1:9222';
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
const STEP_REPEAT_MS = 15_000;
const ADMIT_INTERVAL_MS = 5_000;
const WEB_JOIN_STEPS = ['Continue on this browser', 'Not now', '=Join now', '=Join'];
const GUEST_NAME = 'Call e2e guest';
const LOBBY_TEXTS = ['let you in shortly', 'should let you in soon'];
const WEB_THRESHOLD = { speech: 0.005, floor: 0.0005 };
const APP_THRESHOLD = { speech: 200, floor: 20 };
const SPEECH_TO_PAUSE = 4;
const SCENARIO_SETTLE_MS = 8_000;
const SCENARIO_MEASURE_MS = 8_000;
const STATS_WAIT_MS = 1_500;
const MIN_FRAMES = 30;

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

const speechDataUrl = () => `data:audio/wav;base64,${readFileSync(PROBE_DIR + '/web.wav').toString('base64')}`;

// Taps every received audio track, so the level does not depend on headless Chrome playing it out.
const REMOTE_AUDIO_TAP = `(() => {
  window.__remoteAudio = { sum: 0, count: 0, packets: 0 };
  const tapped = new WeakSet();
  const tap = async (track) => {
    tapped.add(track);
    const reader = new MediaStreamTrackProcessor({ track }).readable.getReader();
    for (;;) {
      const { value, done } = await reader.read();
      if (done) return;
      const samples = new Float32Array(value.numberOfFrames);
      value.copyTo(samples, { planeIndex: 0, format: 'f32-planar' });
      value.close();
      for (const sample of samples) window.__remoteAudio.sum += sample * sample;
      window.__remoteAudio.count += samples.length;
    }
  };
  setInterval(() => {
    for (const peer of window.__pcs || []) {
      for (const receiver of peer.getReceivers()) {
        if (receiver.track && receiver.track.kind === 'audio' && !tapped.has(receiver.track)) tap(receiver.track).catch(() => {});
      }
    }
  }, 500);
})();`;

const INBOUND_AUDIO = `({ ...window.__remoteAudio })`;

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
const LOCATE = (step) => `(() => {
  const onlyExact = ${JSON.stringify(step.startsWith('='))};
  const wanted = ${JSON.stringify(step.replace(/^=/, ''))}.toLowerCase();
  const nodes = [...document.querySelectorAll('button, [role=button], [role=menuitem], a')];
  const label = (node) => ((node.getAttribute('aria-label') || '') + ' ' + (node.innerText || '') + ' ' + (node.getAttribute('title') || '')).trim().toLowerCase();
  const visible = nodes.filter((node) => node.getClientRects().length && !node.disabled && node.getAttribute('aria-disabled') !== 'true');
  const exact = visible.find((node) => (node.innerText || '').trim().toLowerCase() === wanted || (node.getAttribute('aria-label') || '').trim().toLowerCase() === wanted);
  const target = exact || (onlyExact ? null : visible.find((node) => label(node).includes(wanted)));
  if (!target) return null;
  target.scrollIntoView({ block: 'center' });
  target.click();
  return label(target).slice(0, 80);
})()`;
const CONNECTED = `(window.__pcs || []).some((peer) => peer.connectionState === 'connected')`;
const TEAMS_ORIGINS = ['https://teams.cloud.microsoft', 'https://teams.microsoft.com'];

class GuestTab {
  constructor(browser) { this.browser = browser; }
  async open(link) {
    this.contextId = (await this.browser.send('Target.createBrowserContext', { disposeOnDetach: false })).browserContextId;
    const context = this.contextId ? { browserContextId: this.contextId } : {};
    const { targetId } = await this.browser.send('Target.createTarget', { url: 'about:blank', ...context });
    this.targetId = targetId;
    const { sessionId } = await this.browser.send('Target.attachToTarget', { targetId, flatten: true });
    this.sessionId = sessionId;
    await this.browser.send('Page.enable', {}, sessionId);
    await this.browser.send('Runtime.enable', {}, sessionId);
    await this.browser.send('Page.addScriptToEvaluateOnNewDocument', { source: PEER_HOOK }, sessionId);
    await this.browser.send('Page.addScriptToEvaluateOnNewDocument', { source: REMOTE_AUDIO_TAP }, sessionId);
    await this.browser.send('Page.addScriptToEvaluateOnNewDocument', { source: FAKE_MEDIA(speechDataUrl()) }, sessionId);
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
    const clickedAt = new Map();
    while (Date.now() < deadline) {
      const inLobby = await this.evaluate(`${JSON.stringify(LOBBY_TEXTS)}.some((text) => document.body.innerText.includes(text))`).catch(() => false);
      if (!inLobby && (await this.evaluate(CONNECTED).catch(() => false))) return true;
      await this.activate();
      await this.typeName();
      for (const step of WEB_JOIN_STEPS) {
        if (Date.now() - (clickedAt.get(step) ?? 0) < STEP_REPEAT_MS) continue;
        if (await this.click(step)) {
          clickedAt.set(step, Date.now());
          log(`web: clicked "${step}"`);
        }
      }
      await delay(2000);
    }
    return false;
  }
  async click(step) {
    return this.evaluate(LOCATE(step)).catch(() => null);
  }
  async activate() {
    for (const type of ['mousePressed', 'mouseReleased']) {
      await this.browser.send('Input.dispatchMouseEvent', { type, x: 5, y: 5, button: 'left', clickCount: 1 }, this.sessionId).catch(() => {});
    }
  }
  async typeName() {
    const focused = await this.evaluate(`(() => {
      const input = document.querySelector('input[placeholder*="name" i]');
      if (!input || input.value) return false;
      input.focus(); return true;
    })()`).catch(() => false);
    if (focused) await this.browser.send('Input.insertText', { text: GUEST_NAME }, this.sessionId);
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
    const count = current.count - previous.count;
    levels.push(count > 0 ? Math.sqrt((current.sum - previous.sum) / count) : 0);
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


async function appStats(workDir) {
  const path = workDir + '/app-stats.json';
  writeFileSync(path, '');
  runTeams('--call', `stats ${windowsPath(path)}`);
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    await delay(STATS_WAIT_MS);
    const text = readFileSync(path, 'utf8');
    if (text) return JSON.parse(text);
  }
  throw new Error('the app wrote no stats');
}

const sum = (entries, key) => entries.reduce((total, entry) => total + (entry[key] || 0), 0);

async function guestVideo(tab) {
  const stats = await tab.evaluate(PEER_STATS);
  return {
    decoded: sum(stats.videoIn, 'decoded'),
    widest: Math.max(0, ...stats.videoIn.map((entry) => entry.width || 0)),
    sentFrames: sum(stats.videoOut, 'frames'),
  };
}

async function measure(tab, workDir) {
  const [appBefore, guestBefore] = [await appStats(workDir), await guestVideo(tab)];
  await delay(SCENARIO_MEASURE_MS);
  const [appAfter, guestAfter] = [await appStats(workDir), await guestVideo(tab)];
  const grew = (after, before, key) => (after[key] || 0) - (before[key] || 0);
  return {
    app: appAfter,
    appVideoOut: grew(appAfter, appBefore, 'videoOutFramesEncoded'),
    appVideoIn: grew(appAfter, appBefore, 'videoInFramesDecoded'),
    appVideoShown: grew(appAfter, appBefore, 'videoFramesPublished'),
    appShareOut: grew(appAfter, appBefore, 'shareOutPackets'),
    appShareIn: grew(appAfter, appBefore, 'shareInPackets'),
    guestVideoIn: guestAfter.decoded - guestBefore.decoded,
    guestWidest: guestAfter.widest,
    guestVideoOut: guestAfter.sentFrames - guestBefore.sentFrames,
  };
}

const ONLY = process.env.SCENARIOS?.split(',');

async function scenario(name, report, tab, workDir, act, check) {
  if (ONLY && !ONLY.includes(name)) return;
  log(`scenario: ${name}`);
  try {
    await act();
    await delay(SCENARIO_SETTLE_MS);
    const measured = await measure(tab, workDir);
    const pass = Boolean(check(measured));
    report.scenarios[name] = { pass, ...measured, app: undefined, appState: measured.app };
    log(`scenario ${name}: ${pass ? 'PASS' : 'FAIL'}`);
    if (!pass) {
      await tab.shot(`${workDir}/${name}.png`).catch(() => {});
      report.scenarios[name].guestVideoStreams = (await tab.evaluate(PEER_STATS).catch(() => ({}))).videoIn;
    }
  } catch (error) {
    report.scenarios[name] = { pass: false, error: error.message };
    log(`scenario ${name}: ERROR ${error.message}`);
  }
}

async function guestClick(tab, labels) {
  for (const label of labels) {
    if (await tab.click(label)) return label;
  }
  throw new Error(`guest found none of ${labels.join(', ')}`);
}

async function guestCamera(tab, on) {
  const sending = (await guestVideo(tab)).sentFrames;
  await delay(2_000);
  const isOn = (await guestVideo(tab)).sentFrames > sending;
  if (isOn !== on) await guestClick(tab, [on ? '=Turn camera on' : '=Turn camera off', '=Camera']);
}

async function runScenarios(report, tab, workDir) {
  report.scenarios = {};
  await scenario('app-camera-on', report, tab, workDir, () => runTeams('--call', 'camera on'),
    (m) => m.app.cameraOn && m.appVideoOut >= MIN_FRAMES && m.guestVideoIn >= MIN_FRAMES);
  await scenario('guest-camera-on', report, tab, workDir, () => guestCamera(tab, true),
    (m) => m.guestVideoOut >= MIN_FRAMES && m.appVideoIn >= MIN_FRAMES && m.appVideoShown >= MIN_FRAMES);
  await scenario('app-camera-off', report, tab, workDir, () => runTeams('--call', 'camera off'),
    (m) => !m.app.cameraOn && m.appVideoOut < MIN_FRAMES && m.appVideoIn >= MIN_FRAMES);
  await scenario('guest-camera-off', report, tab, workDir, () => guestCamera(tab, false),
    (m) => m.appVideoIn < MIN_FRAMES);
  await scenario('app-share', report, tab, workDir, async () => {
    runTeams('--call', 'share-sources');
    await delay(3_000);
    runTeams('--call', 'share');
  }, (m) => m.app.localShare && m.appShareOut > 50 && m.guestVideoIn >= MIN_FRAMES);
  await scenario('app-stop-share', report, tab, workDir, () => runTeams('--call', 'stop-share'),
    (m) => !m.app.localShare && m.appShareOut < 20);
  await scenario('guest-share', report, tab, workDir, async () => {
    await guestClick(tab, ['=Share', 'Share content', 'Share']);
    await delay(2_000);
    await guestClick(tab, ['=Screen', 'Share screen', 'Screen', 'Entire screen']).catch(() => null);
  }, (m) => m.app.screenSharer && m.appShareIn > 50);
  await scenario('guest-stop-share', report, tab, workDir, () => guestClick(tab, ['=Stop sharing', 'Stop sharing', 'Stop presenting']),
    (m) => !m.app.screenSharer && m.appShareIn < 20);
  await scenario('app-mute', report, tab, workDir, () => runTeams('--call', 'mute'), (m) => m.app.muted);
  const muted = await sampleWeb(tab, 6);
  report.scenarios['app-mute'].guestHeardWhileMuted = judge(muted, WEB_THRESHOLD);
  if (report.scenarios['app-mute'].guestHeardWhileMuted.p90 >= WEB_THRESHOLD.speech) report.scenarios['app-mute'].pass = false;
  await scenario('app-unmute', report, tab, workDir, () => runTeams('--call', 'unmute'), (m) => !m.app.muted);
  const unmuted = await sampleWeb(tab, 8);
  report.scenarios['app-unmute'].guestHears = judge(unmuted, WEB_THRESHOLD);
  if (!report.scenarios['app-unmute'].guestHears.pass) report.scenarios['app-unmute'].pass = false;
}

const report = { started: new Date().toISOString() };
const savedCapture = defaultAudio('get', 'capture');
const restoreCapture = () => defaultAudio('set', 'capture', savedCapture);
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => { restoreCapture(); process.exit(130); });
const browser = await new Browser(OWN_ENDPOINT).connect();
const tab = new GuestTab(browser);
try {
  if (!teamsProcessId()) {
    log(`starting the app, waiting ${APP_START_WAIT_MS / 1000}s`);
    runTeams();
    await delay(APP_START_WAIT_MS);
  }
  const link = givenLink ?? createMeeting();
  report.link = link;
  log('meeting ready');
  defaultAudio('set', 'capture', 'CABLE Output');
  log('default mic: CABLE Output');
  const playSeconds = Math.ceil((APP_JOIN_WAIT_MS + WEB_JOIN_TIMEOUT_MS) / 1000) + MEASURE_SECONDS + 10 + (process.env.AUDIO_ONLY ? 0 : 240);
  const playing = runProbe('play', 'CABLE Input', windowsPath(PROBE_DIR + '/app.wav'), String(playSeconds));
  runTeams('--join', link);
  log(`app joining, waiting ${APP_JOIN_WAIT_MS / 1000}s`);
  await delay(APP_JOIN_WAIT_MS);
  await tab.open(link);
  const admitting = setInterval(() => runTeams('--admit-all'), ADMIT_INTERVAL_MS);
  report.webJoined = await tab.join().finally(() => clearInterval(admitting));
  log(`web guest joined: ${report.webJoined}`);
  if (!report.webJoined) {
    await tab.shot(workDir + '/web-join.png');
    throw new Error('Teams web guest did not join, see web-join.png');
  }
  await tab.activate();
  runTeams('--unmute');
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
  if (!process.env.AUDIO_ONLY) await runScenarios(report, tab, workDir);
  await playing.catch((error) => (report.playError = error.message));
} catch (error) {
  report.error = error.message;
} finally {
  if (teamsProcessId()) runTeams('--hang-up');
  await tab.close();
  restoreCapture();
  log('default mic restored');
  browser.socket.close();
}
const scenariosPass = Object.values(report.scenarios ?? {}).every((entry) => entry.pass);
report.pass = Boolean(report.appToWeb?.pass && report.webToApp?.pass && scenariosPass);
writeFileSync(workDir + '/report.json', JSON.stringify(report, null, 2));
const scenarioSummary = Object.fromEntries(Object.entries(report.scenarios ?? {}).map(([name, entry]) => [name, entry.pass ? 'pass' : `FAIL ${entry.error ?? ''}`.trim()]));
console.log(JSON.stringify({ pass: report.pass, appToWeb: report.appToWeb, webToApp: report.webToApp, scenarios: scenarioSummary, guestSending: report.guestSending, error: report.error }));
process.exit(report.pass ? 0 : 1);
