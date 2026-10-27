// Renders ad.html frame by frame through headless Edge and pipes the PNGs into ffmpeg.
//   node render.js                      -> out/video.mp4 (60 fps, silent)
//   node render.js --stills 1,3.9,7.2   -> out/still-<t>.png
// Env: PUPPETEER_DIR (folder with node_modules/puppeteer-core), FFMPEG (ffmpeg binary), BROWSER (Edge/Chrome path).
const path = require('path'), fs = require('fs'), { spawn } = require('child_process');
const puppeteer = require(path.join(process.env.PUPPETEER_DIR || '.', 'node_modules', 'puppeteer-core'));
const FPS = 60, DUR = 15, OUT = path.join(__dirname, 'out');
const BROWSER = process.env.BROWSER || 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';

(async () => {
  fs.mkdirSync(OUT, { recursive: true });
  const browser = await puppeteer.launch({ executablePath: BROWSER, headless: true, args: ['--allow-file-access-from-files'] });
  const page = await browser.newPage();
  await page.setViewport({ width: 1920, height: 1080 });
  await page.goto('file:///' + path.join(__dirname, 'ad.html').replace(/\\/g, '/'));
  await page.evaluate(() => window.ready);
  const frame = t => page.evaluate(t => { window.render(t); return document.getElementById('c').toDataURL('image/png'); }, t)
    .then(u => Buffer.from(u.slice(22), 'base64'));

  const si = process.argv.indexOf('--stills');
  if (si > 0) {
    for (const t of process.argv[si + 1].split(',').map(Number)) fs.writeFileSync(path.join(OUT, `still-${t}.png`), await frame(t));
  } else {
    const ff = spawn(process.env.FFMPEG || 'ffmpeg', ['-y', '-loglevel', 'error', '-f', 'image2pipe', '-framerate', String(FPS), '-i', '-',
      '-c:v', 'libx264', '-preset', 'slow', '-crf', '14', '-pix_fmt', 'yuv420p', path.join(OUT, 'video.mp4')], { stdio: ['pipe', 'inherit', 'inherit'] });
    for (let i = 0; i < FPS * DUR; i++) {
      if (!ff.stdin.write(await frame(i / FPS))) await new Promise(r => ff.stdin.once('drain', r));
      if (i % 60 === 0) process.stdout.write(`${i / FPS}s `);
    }
    ff.stdin.end();
    await new Promise(r => ff.on('close', r));
  }
  await browser.close();
})();
