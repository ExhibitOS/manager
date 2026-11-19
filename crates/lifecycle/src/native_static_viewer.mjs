// SPDX-License-Identifier: Apache-2.0
import {createServer} from 'node:http';
import {createInterface} from 'node:readline';
import {once} from 'node:events';
import {extname} from 'node:path';
import {pathToFileURL} from 'node:url';

const input = createInterface({input: process.stdin, crlfDelay: Infinity});
let stopped = false;
let server;
const close = () => {
    stopped = true;
    server?.close();
    server?.closeAllConnections();
};
input.once('close', close);

const mediaTypes = {
    '.html': 'text/html; charset=utf-8', '.css': 'text/css; charset=utf-8',
    '.js': 'text/javascript; charset=utf-8', '.mjs': 'text/javascript; charset=utf-8',
    '.json': 'application/json', '.txt': 'text/plain; charset=utf-8',
    '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.webp': 'image/webp',
    '.avif': 'image/avif', '.svg': 'image/svg+xml', '.glb': 'model/gltf-binary',
    '.woff': 'font/woff', '.woff2': 'font/woff2', '.wasm': 'application/wasm',
    '.wav': 'audio/wav', '.ogg': 'audio/ogg', '.mp3': 'audio/mpeg',
    '.mp4': 'video/mp4', '.webm': 'video/webm',
};

try {
    const [line] = await once(input, 'line');
    const {receipt, secret} = JSON.parse(line);
    const {localStaticPublicationSource} = await import(pathToFileURL(process.argv[1]).href);
    const source = await localStaticPublicationSource({
        sourceDir: receipt.output, sourceManifestSha256: receipt.manifestSha256,
        indexSha256: receipt.indexSha256, publicationId: receipt.publicationId,
        revisionSha256: receipt.revisionSha256,
    }, {cancelled: () => stopped});
    if (stopped || !source.entries.has('index.html')) throw Error('viewer');
    let host;
    const cookie = 'exhibitos_preview=' + secret;
    server = createServer({headersTimeout: 5000, requestTimeout: 10000}, async (request, response) => {
        response.setHeader('Cache-Control', 'no-store');
        response.setHeader('Referrer-Policy', 'no-referrer');
        response.setHeader('X-Content-Type-Options', 'nosniff');
        response.setHeader('Content-Security-Policy', "connect-src 'self' blob:; frame-ancestors 'none'; form-action 'none'; base-uri 'none'");
        const signed = request.headers.authorization === 'Bearer ' + secret;
        const bootstrap = signed && request.url === '/index.html' && request.method === 'GET';
        const admitted = request.headers.cookie?.split(/;\s*/).includes(cookie);
        if (stopped || request.headers.host !== host ||
            (request.headers.origin !== undefined && request.headers.origin !== 'http://' + host) ||
            (!signed && !admitted)) {
            response.writeHead(403).end();
            return;
        }
        if (!['GET', 'HEAD'].includes(request.method)) {
            response.writeHead(405, {Allow: 'GET, HEAD'}).end();
            return;
        }
        const path = request.url === '/' ? 'index.html' : request.url?.slice(1);
        if (!source.entries.has(path) || (path.endsWith('.html') && path !== 'index.html')) {
            response.writeHead(404).end();
            return;
        }
        try {
            const bytes = await source.read(path);
            if (stopped) { response.destroy(); return; }
            if (bootstrap) response.setHeader('Set-Cookie', cookie + '; Path=/; HttpOnly; SameSite=Strict');
            response.writeHead(200, {'Content-Type': mediaTypes[extname(path)] ?? 'application/octet-stream',
                'Content-Length': bytes.length});
            response.end(request.method === 'HEAD' ? undefined : bytes);
        } catch {
            response.writeHead(409).end();
        }
    });
    server.maxConnections = 12;
    server.listen(0, '127.0.0.1');
    await once(server, 'listening');
    if (stopped) throw Error('viewer');
    host = '127.0.0.1:' + server.address().port;
    process.stdout.write(String(server.address().port) + '\n');
} catch {
    close();
    input.close();
    process.exitCode = 1;
}
