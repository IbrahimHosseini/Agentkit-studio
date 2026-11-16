// Private stdio bridge. Pages never receive the Studio IPC API or host credentials.
import { chromium } from 'playwright-core';
import { access } from 'node:fs/promises';
import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';

export function browserUrl(value) {
  if (value === 'about:blank') return value;
  const url = new URL(value);
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password)
    throw new Error('Use an HTTP or HTTPS URL without embedded credentials.');
  return url.href;
}
export function boundedNumber(value, fallback, min, max) {
  const n = value === undefined ? fallback : value;
  if (typeof n !== 'number' || !Number.isFinite(n) || n < min || n > max)
    throw new Error(`Expected a number between ${min} and ${max}.`);
  return n;
}
async function executable() {
  const candidates = [process.env.AGENTKIT_BROWSER_BIN,
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
    '/Applications/Chromium.app/Contents/MacOS/Chromium',
    '/usr/bin/chromium', '/usr/bin/chromium-browser', '/usr/bin/google-chrome',
    process.env.PROGRAMFILES && `${process.env.PROGRAMFILES}/Google/Chrome/Application/chrome.exe`,
    process.env['PROGRAMFILES(X86)'] && `${process.env['PROGRAMFILES(X86)']}/Microsoft/Edge/Application/msedge.exe`,
    chromium.executablePath()].filter(Boolean);
  for (const path of candidates) { try { await access(path); return path; } catch {} }
  throw new Error('Install Chrome, Chromium or Edge to use Browser, or set AGENTKIT_BROWSER_BIN to its executable.');
}

export function createBridge() {
  let browser, context, active, nextId = 0;
  const pages = new Map();
  const logs = [];
  function log(type, text) { logs.push({ type, text: text.slice(0, 4000) }); if (logs.length > 200) logs.shift(); }
  function register(page) {
    if (pages.size >= 8) { void page.close(); return; }
    const id = String(++nextId);
    pages.set(id, page); active = id;
    page.on('console', message => log(message.type(), message.text()));
    page.on('pageerror', error => log('error', error.message));
    page.on('requestfailed', request => log('network', `${request.url()} ${request.failure()?.errorText ?? ''}`));
    page.on('dialog', dialog => { log('dialog', dialog.message()); void dialog.dismiss(); });
    page.on('close', () => { pages.delete(id); if (active === id) active = [...pages.keys()].at(-1); });
    return id;
  }
  async function start() {
    if (browser) return;
    browser = await chromium.launch({ executablePath: await executable(), headless: true, chromiumSandbox: true, timeout: 15000 });
    context = await browser.newContext({ viewport: { width: 1024, height: 768 }, acceptDownloads: false, serviceWorkers: 'block' });
    context.setDefaultTimeout(8000);
    context.setDefaultNavigationTimeout(15000);
    // Do not allow navigation to file:// or other privileged schemes, including redirects/popups.
    await context.route('**/*', route => {
      try { browserUrl(route.request().url()); return route.continue(); }
      catch { return route.abort('blockedbyclient'); }
    });
    context.on('page', register);
  }
  async function list() {
    return { pages: await Promise.all([...pages].map(async ([id, page]) => ({ id, url: page.url(), title: await page.title().catch(() => '') }))), active_page_id: active ?? null };
  }
  async function dispatch(action, p = {}) {
    if (action === 'status') {
      try { return { available: true, executable: await executable(), running: !!browser }; }
      catch (e) { return { available: false, reason: e.message, running: false }; }
    }
    if (action === 'list') return list();
    if (action === 'open') {
      const url = browserUrl(p.url ?? 'about:blank');
      if (pages.size >= 8) throw new Error('Close a browser tab before opening another (limit: 8).');
      await start();
      const page = await context.newPage();
      await page.goto(url, { waitUntil: 'domcontentloaded' });
      return list();
    }
    if (action === 'close' && !p.page_id) { await context?.close(); await browser?.close(); browser = context = undefined; pages.clear(); active = undefined; return list(); }
    const id = p.page_id ?? active;
    const page = pages.get(id);
    if (!page) throw new Error('Open a browser tab first.');
    active = id;
    switch (action) {
      case 'navigate': await page.goto(browserUrl(p.url), { waitUntil: 'domcontentloaded' }); break;
      case 'back': await page.goBack({ waitUntil: 'domcontentloaded' }); break;
      case 'forward': await page.goForward({ waitUntil: 'domcontentloaded' }); break;
      case 'reload': await page.reload({ waitUntil: 'domcontentloaded' }); break;
      case 'close': await page.close(); break;
      case 'resize': await page.setViewportSize({ width: Math.round(boundedNumber(p.width, 1024, 320, 1920)), height: Math.round(boundedNumber(p.height, 768, 240, 1440)) }); break;
      case 'click': {
        const viewport = page.viewportSize();
        await page.mouse.click(boundedNumber(p.x, 0, 0, viewport.width), boundedNumber(p.y, 0, 0, viewport.height), { button: p.button === 'right' ? 'right' : 'left', clickCount: p.double ? 2 : 1 }); break;
      }
      case 'type': if (typeof p.text !== 'string' || p.text.length > 16000) throw new Error('Text exceeds 16000 characters.'); await page.keyboard.insertText(p.text); break;
      case 'key': if (typeof p.key !== 'string' || p.key.length > 80) throw new Error('Invalid key.'); await page.keyboard.press(p.key); break;
      case 'scroll': await page.mouse.wheel(boundedNumber(p.delta_x, 0, -5000, 5000), boundedNumber(p.delta_y, 0, -5000, 5000)); break;
      case 'console': return { entries: logs };
      case 'snapshot': return { page_id: id, url: page.url(), text: (await page.locator('body').innerText()).slice(0, 64000), accessibility: (await page.locator('body').ariaSnapshot()).slice(0, 64000) };
      case 'frame': {
        const bytes = await page.screenshot({ type: 'jpeg', quality: 70, timeout: 5000 });
        return { page_id: id, url: page.url(), title: await page.title(), mime_type: 'image/jpeg', data: bytes.toString('base64'), ...page.viewportSize() };
      }
      default: throw new Error(`Unknown browser action: ${action}`);
    }
    return list();
  }
  return { dispatch, close: () => dispatch('close') };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const bridge = createBridge();
  const input = createInterface({ input: process.stdin, crlfDelay: Infinity });
  for await (const line of input) {
    try {
      if (line.length > 128 * 1024) throw new Error('Browser request too large.');
      const { action, params } = JSON.parse(line);
      const result = await bridge.dispatch(action, params);
      process.stdout.write(JSON.stringify({ result }) + '\n');
    } catch (error) { process.stdout.write(JSON.stringify({ error: error.message }) + '\n'); }
  }
  await bridge.close();
}
