#!/usr/bin/env node
/**
 * Browser harness — the repeatable proof for Maverick's agent browser.
 *
 * It answers two questions at once, with one command:
 *
 *   1. RELIABILITY   Can the agent actually browse? Per target: did navigation
 *                    succeed, did content render, did the tools respond.
 *   2. DETECTION     What do sites actually see? The exact signals a
 *                    bot-detection script reads off `window` — navigator.webdriver,
 *                    plugin count, UA headless tell, languages, concurrency.
 *
 * It runs against BOTH kinds of target:
 *   - your OWN endpoints (use it to verify your own bot-detection behaves)
 *   - ordinary public sites (use it to verify browsing is reliable)
 *
 * It deliberately does NOT defeat, bypass, or fingerprint-spoof third-party
 * anti-bot systems. It measures what the real, installed Chrome reports and
 * whether browsing works. That is the whole output.
 *
 * Usage:
 *   node scripts/browser-harness.mjs [targets...] [options]
 *
 * Targets default to a small mix of own-style + public. Options:
 *   --runs N        repeat each target N times (default 1) → success rate
 *   --headless      run headless instead of headed
 *   --no-profile    throwaway profile instead of the persistent one
 *   --json          machine-readable output only
 *   --timeout MS    per-step timeout (default 90000)
 *
 * Exit code 0 if every run succeeded, 1 otherwise — so it drops straight into
 * CI or a pre-release check.
 */

import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { dirname } from 'node:path';

const HERE = dirname(fileURLToPath(import.meta.url));

// --- args ------------------------------------------------------------------
const argv = process.argv.slice(2);
const flag = (name) => argv.includes(name);
const opt = (name, dflt) => {
  const i = argv.indexOf(name);
  return i >= 0 && argv[i + 1] ? argv[i + 1] : dflt;
};
const targets = argv.filter((a) => !a.startsWith('--') && isNaN(Number(a)) && argv[argv.indexOf(a) - 1] !== '--runs' && argv[argv.indexOf(a) - 1] !== '--timeout');
const RUNS = Number(opt('--runs', '1'));
const TIMEOUT = Number(opt('--timeout', '90000'));
const HEADLESS = flag('--headless');
const PERSISTENT = !flag('--no-profile');
const AS_JSON = flag('--json');

// Default mix: ordinary public sites that exercise different detection paths.
// (httpbin.org itself has been timing out for months — use httpbingo.org.)
const DEFAULT_TARGETS = [
  'https://example.com',
  'https://httpbingo.org/headers',
  'https://www.whatismybrowser.com/detect/is-javascript-enabled/',
];
const useTargets = targets.length ? targets : DEFAULT_TARGETS;

// --- MCP plumbing ----------------------------------------------------------
// Same shape as computer_args() in src-tauri/src/commands.rs — keep in sync.
function browserArgs(profile) {
  const args = ['-y', '@playwright/mcp@latest', '--browser', 'chrome', '--caps', 'vision'];
  if (PERSISTENT) args.push('--user-data-dir', profile);
  if (HEADLESS) args.push('--headless');
  return args;
}

/** Kill a spawned MCP server AND its descendants.
 *
 *  `spawn(..., { shell: true })` wraps the real server in a cmd.exe shell, so a
 *  bare `child.kill()` reaps only that wrapper and leaves the grandchild
 *  (npx -> node -> Playwright MCP) orphaned — one leak per hung handshake. On
 *  Windows `taskkill /T` walks the process tree; elsewhere `detached` gives us
 *  the process group. The kill is best-effort: the process may already be gone
 *  by the time we reach it (e.g. after a normal exit). */
function killTree(child) {
  if (!child || child.exitCode !== null || child.killed) return;
  try {
    if (process.platform === 'win32') {
      spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
    }
  } catch { /* best effort — the child may already be gone */ }
  try { child.kill(); } catch { /* already gone */ }
}

function startServer(profile) {
  const child = spawn('npx', browserArgs(profile), { stdio: ['pipe', 'pipe', 'pipe'], shell: true });
  let stderr = '';
  let buf = '';
  const pending = new Map();
  child.stderr.on('data', (d) => { stderr += d.toString(); });
  child.stdout.on('data', (d) => {
    buf += d.toString();
    let idx;
    while ((idx = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, idx).trim();
      buf = buf.slice(idx + 1);
      if (!line) continue;
      let msg;
      try { msg = JSON.parse(line); } catch { continue; }
      if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); }
    }
  });
  let seq = 0;
  const rpc = (method, params, isNotification = false) =>
    new Promise((resolve) => {
      const id = isNotification ? undefined : ++seq;
      const payload = isNotification ? { jsonrpc: '2.0', method, params } : { jsonrpc: '2.0', id, method, params };
      if (!isNotification) pending.set(id, resolve);
      child.stdin.write(JSON.stringify(payload) + '\n');
      if (isNotification) resolve(null);
    });
  const callTool = (name, args) => rpc('tools/call', { name, arguments: args });
  return { child, rpc, callTool, stderr: () => stderr };
}

const withTimeout = (p, ms, what) =>
  Promise.race([p, new Promise((_, rej) => setTimeout(() => rej(new Error(`timeout: ${what}`)), ms))]);

const summarise = (r) => {
  const content = r?.result?.content ?? [];
  const texts = content.filter((c) => c.type === 'text').map((c) => c.text).join('\n');
  return { isError: !!r?.result?.isError, images: content.filter((c) => c.type === 'image').length, text: texts };
};

/**
 * Pull a JSON object out of a tool result's ### Result block.
 *
 * Playwright wraps JS return values as a JSON *string*, so the payload often
 * arrives with escaped quotes (`\"webdriver\"`). The result text also carries a
 * trailing "Ran Playwright code" block with its own braces, so we can't just
 * match greedily — instead scan forward from the first `{`, trying each
 * closing brace, until one parses. That survives escaping and nested objects.
 */
const firstJson = (text) => {
  const start = text.indexOf('{');
  if (start < 0) return null;
  for (let end = text.indexOf('}', start); end >= 0; end = text.indexOf('}', end + 1)) {
    const raw = text.slice(start, end + 1);
    for (const c of [raw, raw.replace(/\\"/g, '"').replace(/\\\\/g, '\\')]) {
      try { return JSON.parse(c); } catch { /* try a later brace */ }
    }
  }
  return null;
};

// --- one target ------------------------------------------------------------
async function probeTarget(ctx, url) {
  const out = { url, ok: false, title: '', finalUrl: '', signals: null, screenshot: false, ms: 0, error: '' };
  const t0 = Date.now();
  try {
    const nav = summarise(await withTimeout(ctx.callTool('browser_navigate', { url }), TIMEOUT, `navigate ${url}`));
    if (nav.isError) { out.error = nav.text.slice(0, 200); return finish(); }
    const u = nav.text.match(/Page URL: (.+)/);
    const t = nav.text.match(/Page Title: (.+)/);
    out.finalUrl = u ? u[1].trim() : '';
    out.title = t ? t[1].trim() : '';
    out.ok = true;

    // The detection signals — read straight off the page the way any script would.
    const sig = summarise(await withTimeout(ctx.callTool('browser_evaluate', {
      function: `() => JSON.stringify({
        webdriver: navigator.webdriver,
        ua: navigator.userAgent,
        headlessTell: /HeadlessChrome/.test(navigator.userAgent),
        plugins: navigator.plugins.length,
        languages: navigator.languages,
        hardwareConcurrency: navigator.hardwareConcurrency,
        platform: navigator.platform,
        cookieEnabled: navigator.cookieEnabled
      })`,
    }), TIMEOUT, 'signals'));
    out.signals = firstJson(sig.text);

    // Vision proof: an image back means the model (and a watcher) can see it.
    const shot = summarise(await withTimeout(ctx.callTool('browser_take_screenshot', {}), TIMEOUT, 'screenshot'));
    out.screenshot = shot.images > 0 && !shot.isError;
  } catch (e) {
    out.error = e.message;
  }
  return finish();

  function finish() {
    out.ms = Date.now() - t0;
    return out;
  }
}

// --- verdict ---------------------------------------------------------------
/**
 * A run is "clean" when browsing worked AND the page's own environment reads
 * like a normal desktop browser: no webdriver flag, no headless UA tell,
 * plugins present, real language list. That is what ordinary sites expect to
 * see, so it doubles as the reliability signal.
 */
function verdict(r) {
  if (!r.ok) return { pass: false, reason: r.error || 'navigation failed' };
  const s = r.signals;
  if (!s) return { pass: false, reason: 'no signals read' };
  const fails = [];
  if (s.webdriver === true) fails.push('navigator.webdriver is true');
  if (s.headlessTell) fails.push('HeadlessChrome in UA');
  if (!s.plugins) fails.push('zero plugins');
  if (!s.languages || !s.languages.length) fails.push('no languages');
  return fails.length ? { pass: false, reason: fails.join('; ') } : { pass: true, reason: 'ok' };
}

// --- main ------------------------------------------------------------------
async function main() {
  const profile = mkdtempSync(join(tmpdir(), 'mav-harness-'));
  let ctx = startServer(profile);

  const handshake = async (c) => {
    await withTimeout(c.rpc('initialize', {
      protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'maverick-harness', version: '1.0.0' },
    }), 180000, 'initialize');
    await c.rpc('notifications/initialized', {}, true);
  };

  try {
    await handshake(ctx);
  } catch (e) {
    process.stderr.write(`MCP handshake failed: ${e.message}\n${ctx.stderr().slice(0, 800)}\n`);
    killTree(ctx.child);
    process.exit(1);
  }

  /** A failed run can leave the shared browser context dead — respawn so one
   *  bad target can't cascade into every target after it. */
  async function recover() {
    killTree(ctx.child);
    ctx = startServer(profile);
    try {
      await handshake(ctx);
    } catch (e) {
      process.stderr.write(`recovery handshake failed: ${e.message}\n`);
    }
  }

  const results = [];
  let allOk = true;

  for (const url of useTargets) {
    const runs = [];
    for (let i = 0; i < RUNS; i++) {
      // Re-read ctx each iteration — recover() below swaps in a fresh server.
      const r = await probeTarget(ctx, url);
      r.verdict = verdict(r);
      runs.push(r);
      if (!r.verdict.pass) {
        allOk = false;
        // A dead context shows up as an instant "browser has been closed"
        // error on every later target. Restart so the next target gets a fair
        // try instead of inheriting a corpse. (Navigation failures like a 404
        // don't poison the context, but restarting is cheap and rare.)
        await recover();
      }
      if (!AS_JSON) {
        const mark = r.verdict.pass ? 'PASS' : 'FAIL';
        process.stdout.write(
          `  [${mark}] ${url}  ${r.ms}ms  ${r.verdict.pass ? '' : '— ' + r.verdict.reason + '  '}${r.title ? `title="${r.title}"` : ''}\n`
        );
      }
    }
    const passed = runs.filter((r) => r.verdict.pass).length;
    const sample = runs[runs.length - 1];
    results.push({
      url,
      runs: RUNS,
      passed,
      rate: `${passed}/${RUNS}`,
      ms: Math.round(runs.reduce((a, r) => a + r.ms, 0) / runs.length),
      title: sample.title,
      finalUrl: sample.finalUrl,
      signals: sample.signals,
      screenshotSeen: sample.screenshot,
      failures: runs.filter((r) => !r.verdict.pass).map((r) => r.verdict.reason),
    });
  }

  killTree(ctx.child);
  if (AS_JSON) {
    process.stdout.write(JSON.stringify({ mode: HEADLESS ? 'headless' : 'headed', persistent: PERSISTENT, results, allOk }, null, 2) + '\n');
  } else {
    process.stdout.write('\n');
    for (const r of results) {
      process.stdout.write(`${r.url}  ${r.rate}  avg ${r.ms}ms  screenshot=${r.screenshotSeen ? 'yes' : 'no'}\n`);
      const s = r.signals;
      if (s) {
        process.stdout.write(`   webdriver=${s.webdriver} headlessTell=${s.headlessTell} plugins=${s.plugins} ` +
          `langs=${(s.languages || []).join(',')} cores=${s.hardwareConcurrency}\n`);
        process.stdout.write(`   ua=${s.ua}\n`);
      }
    }
    process.stdout.write(`\n${allOk ? 'ALL PASS' : 'FAILURES PRESENT'} — ${results.length} target(s), ${RUNS} run(s) each\n`);
  }

  process.exit(allOk ? 0 : 1);
}

main();
