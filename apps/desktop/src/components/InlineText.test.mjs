import test, { after } from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, writeFile, rm } from 'node:fs/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';

// Compile the actual component: assertions cover escaped HTML and label markup,
// rather than duplicating the parser in the test.
const directory = await mkdtemp(fileURLToPath(new URL('../../node_modules/.inline-text-test-', import.meta.url)));
after(() => rm(directory, { recursive: true, force: true }));
const source = new URL('./InlineText.svelte', import.meta.url);
const { js } = compile(await readFile(source, 'utf8'), { filename: fileURLToPath(source), generate: 'server' });
const compiled = `${directory}/InlineText.mjs`;
await writeFile(compiled, js.code);
const { default: InlineText } = await import(pathToFileURL(compiled).href);
const html = text => render(InlineText, { props: { text } }).body.replace(/<!--[^]*?-->/g, '');
const path = '/Users/ebo/Developer/arcaptcha/Sources/ARCaptcha/ARCaptcha.swift:13';

for (const [name, text] of [
  ['standard', '[`ARCaptcha.verify(...)`](' + path + '),'],
  ['escaped opening delimiter', '[`ARCaptcha.verify(...)`]\\(' + path + '),'],
  ['escaped delimiters', '[`ARCaptcha.verify(...)`]\\(' + path + '\\),'],
]) {
  test(`renders the reported local reference with ${name}`, () => {
    const output = html(text);
    assert.ok(output.includes(`href="${path}"`));
    assert.ok(output.includes(`title="Reveal file: ${path}"`));
    assert.match(output, /<a [^>]+><code dir="ltr">ARCaptcha\.verify\(\.\.\.\)<\/code><\/a>,$/);
    assert.ok(!output.includes('target="_blank"'));
  });
}

test('balanced and escaped destination parentheses do not truncate paths or following text', () => {
  for (const destination of ['/tmp/folder(a(b))/file.swift:13:2', '/tmp/folder\\(a\\)/file.swift:13']) {
    const output = html(`[source](${destination}), then [docs](https://example.com/a_(b(c))).`);
    assert.match(output, /file\.swift:13(?::2)?"/);
    assert.match(output, /<\/a>, then <a href="https:\/\/example.com\/a_\(b\(c\)\)"/);
    assert.ok(output.endsWith('</a>.'));
  }
});

test('angle destinations preserve spaces and Windows drive references', () => {
  assert.match(html('[source](</tmp/My Project/File.swift:9>)'), /href="\/tmp\/My Project\/File.swift:9"/);
  assert.match(html('[source](<C:\\My Project\\File.swift:9:2>)'), /title="Reveal file: C:\\My Project\\File.swift:9:2"/);
});

test('web links keep formatted labels, external-link attributes, and escaped destinations', () => {
  const output = html('[`verify()` and **docs**](https://example.com/a\\(b\\)?x=1&y=2)');
  assert.match(output, /href="https:\/\/example.com\/a\(b\)\?x=1&amp;y=2"/);
  assert.match(output, /target="_blank" rel="noopener noreferrer"/);
  assert.match(output, /<code dir="ltr">verify\(\)<\/code> and <strong>docs<\/strong>/);
});

test('unsafe and unsupported destinations remain text, never actionable links', () => {
  for (const destination of ['javascript:alert(1)', 'data:text/html,hello', '//example.com/file', 'relative.swift:13', '/tmp/file\u0000.swift']) {
    assert.ok(!html(`[source](${destination})`).includes('<a '), destination);
  }
  const output = html('[<img src=x onerror=alert(1)>](/tmp/file.swift:1)');
  assert.ok(!output.includes('<img'));
  assert.match(output, /&lt;img/);
});

test('ordinary code and incomplete streaming Markdown remain readable', () => {
  assert.match(html('`[source](/tmp/file.swift:13)`'), /^<code dir="ltr">\[source\]\(\/tmp\/file.swift:13\)<\/code>$/);
  assert.equal(html('[source](/tmp/file.swift:13'), '[source](/tmp/file.swift:13');
  assert.equal(html('before **bold** and `code` after'), 'before <strong>bold</strong> and <code dir="ltr">code</code> after');
});
