const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const test = require('node:test');
const source = fs.readFileSync(require('node:path').join(__dirname, '../docs/script.js'), 'utf8');
function page(os, release) {
    const elements = new Map();
    const element = id => {
        if (!elements.has(id)) elements.set(id, {style: {}, href: '', textContent: '', innerHTML: '', querySelector: () => element('button-label')});
        return elements.get(id);
    };
    const context = vm.createContext({console, document: {getElementById: element, addEventListener() {}}, window: {navigator: {userAgent: os}}, fetch: async () => {throw Error('offline');}});
    vm.runInContext(source, context);
    vm.runInContext(`gUserOS=${JSON.stringify(os)};gReleaseData=${JSON.stringify(release)};updateDownloadLinks();`, context);
    return {element, context};
}
const release = {tag_name: 'v0.8.0', html_url: 'https://example.test/releases', assets: [
    {name: 'DataBeam_windows-x64.exe', browser_download_url: 'https://example.test/windows'},
    {name: 'DataBeam_linux-arm64.AppImage', browser_download_url: 'https://example.test/linux-arm64'},
]};
test('Android downloads its native beta APK instead of a Linux AppImage', () => {
    const p = page('android', release);
    assert.match(p.element('download-btn').href, /android-v0\.8\.0-beta\.1\/DataBeam_android-arm64_beta\.apk$/);
    assert.equal(p.element('button-label').textContent, 'Download Android beta APK');
    assert.equal(p.element('link-linux-arm64').href, 'https://example.test/linux-arm64');
});
test('Android download works when desktop release API is unavailable', async () => {
    const p = page('android', null);
    await vm.runInContext('fetchLatestRelease()', p.context);
    assert.match(p.element('download-btn').href, /\.apk$/);
    assert.match(p.element('download-status').textContent, /Android 8\.0\+/);
});
test('Windows continues to use the stable desktop asset', () => {
    const p = page('windows', release);
    assert.equal(p.element('download-btn').href, 'https://example.test/windows');
    assert.equal(p.element('button-label').textContent, 'Download for Windows');
});
