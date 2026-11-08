// 1:1 call end-to-end test against the Echo bot with the deployed Windows app: no second person needed.
// The app's mic is VB-CABLE fed with a speech file; the bot plays its prompt, records us and plays it back.
// Checks the app hears the bot and our own speech back, that the call ends when the bot hangs up, and hold/resume in a second call.
// Usage: node scripts/call-echo-e2e.mjs <work dir>
import { spawn, execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';

const [workDir] = process.argv.slice(2);
const PROBE_DIR = process.env.PROBE_DIR ?? '/mnt/c/Users/dd/AppData/Local/Temp/call-e2e';
const TEAMS_EXE = process.env.TEAMS_EXE ?? '/mnt/c/Users/dd/AppData/Local/Programs/ms-teams-linux/teams.exe';
const POWERSHELL = '/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe';
const TASKLIST = '/mnt/c/Windows/System32/tasklist.exe';
const SCRIPT_DIR = new URL('.', import.meta.url).pathname;
const CONNECT_WAIT_MS = 12_000;
const LISTEN_SECONDS = 40;
const HOLD_SETTLE_MS = 6_000;
const HOLD_MEASURE_MS = 6_000;
const PLAY_SECONDS = 200;
const APP_THRESHOLD = { speech: 200, floor: 20 };
const SPEECH_TO_PAUSE = 4;
const RESUMED_PACKETS = 100;
const PROMPT_START_MS = 3_000;
const ENDED_WAIT_MS = 15_000;
const HELD_PACKETS = 60;

const windowsPath = (path) => execFileSync('wslpath', ['-w', path]).toString().trim();
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const log = (message) => console.error(`[${new Date().toISOString().slice(11, 19)}] ${message}`);

function defaultAudio(...args) {
  const script = windowsPath(SCRIPT_DIR + 'default-audio.ps1');
  return execFileSync(POWERSHELL, ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', script, ...args]).toString().trim();
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

const runTeams = (...args) => spawn(TEAMS_EXE, args, { detached: true, stdio: 'ignore' }).unref();

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

async function appStats() {
  const path = workDir + '/app-stats.json';
  writeFileSync(path, '');
  runTeams('--call', `stats ${windowsPath(path)}`);
  for (let attempt = 0; attempt < 6; attempt++) {
    await delay(1_500);
    const text = readFileSync(path, 'utf8');
    if (text) return JSON.parse(text);
  }
  throw new Error('the app wrote no stats');
}

function judge(levels) {
  const sorted = [...levels].sort((left, right) => left - right);
  const percentile = (share) => sorted[Math.min(sorted.length - 1, Math.floor(share * sorted.length))] ?? 0;
  const [p10, p90] = [percentile(0.1), percentile(0.9)];
  return { pass: p90 >= APP_THRESHOLD.speech && p90 >= SPEECH_TO_PAUSE * Math.max(p10, APP_THRESHOLD.floor), p10, p90 };
}

async function mediaDelta(ms) {
  const before = await appStats();
  await delay(ms);
  const after = await appStats();
  return { app: after, sent: after.audioOutPackets - before.audioOutPackets, received: after.audioInPackets - before.audioInPackets };
}

const report = { started: new Date().toISOString(), checks: {} };
const savedCapture = defaultAudio('get', 'capture');
const restoreCapture = () => defaultAudio('set', 'capture', savedCapture);
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => { restoreCapture(); process.exit(130); });
try {
  if (!teamsProcessId()) throw new Error('teams.exe is not running');
  defaultAudio('set', 'capture', 'CABLE Output');
  const playing = runProbe('play', 'CABLE Input', windowsPath(PROBE_DIR + '/app.wav'), String(PLAY_SECONDS));
  const placeEcho = async () => {
    runTeams('--call', 'echo');
    log(`echo call placed, waiting ${CONNECT_WAIT_MS / 1000}s`);
    await delay(CONNECT_WAIT_MS);
    runTeams('--unmute');
    const connected = await appStats();
    if (!(connected.inCall && /Connected/.test(connected.state))) throw new Error(`echo call not connected: ${connected.state}`);
  };
  await placeEcho();
  report.checks.connected = { pass: true };
  log(`listening ${LISTEN_SECONDS}s for the prompt and our echo`);
  const recorded = await runProbe('record-process', String(teamsProcessId()), String(LISTEN_SECONDS), windowsPath(workDir + '/echo-heard.wav'));
  report.checks.heard = judge(JSON.parse(recorded).rms);
  await delay(ENDED_WAIT_MS);
  const after = await appStats();
  report.checks.endsWhenBotLeaves = { pass: !after.inCall, inCall: after.inCall, state: after.state };
  if (after.inCall) runTeams('--hang-up');
  await delay(5_000);
  await placeEcho();
  await delay(PROMPT_START_MS);
  runTeams('--call', 'hold');
  await delay(HOLD_SETTLE_MS);
  const held = await mediaDelta(HOLD_MEASURE_MS);
  report.checks.hold = { pass: held.sent <= HELD_PACKETS && /Local/.test(held.app.hold), sent: held.sent, hold: held.app.hold };
  runTeams('--call', 'resume');
  await delay(HOLD_SETTLE_MS);
  const resumed = await mediaDelta(HOLD_MEASURE_MS);
  report.checks.resume = { pass: resumed.sent >= RESUMED_PACKETS && /Active/.test(resumed.app.hold), sent: resumed.sent, hold: resumed.app.hold };
  await playing.catch((error) => (report.playError = error.message));
} catch (error) {
  report.error = error.message;
} finally {
  if (teamsProcessId()) runTeams('--hang-up');
  restoreCapture();
}
report.pass = !report.error && Object.values(report.checks).every((check) => check.pass);
writeFileSync(workDir + '/echo-report.json', JSON.stringify(report, null, 2));
console.log(JSON.stringify({ pass: report.pass, checks: report.checks, error: report.error }));
process.exit(report.pass ? 0 : 1);
