// Renders the app icon and the menu-bar template icon from the Cat.dc.html soft-clay idle cat.
import { Resvg } from '@resvg/resvg-js';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

const root = join(dirname(new URL(import.meta.url).pathname), '..');
const out = join(root, 'src-tauri/icons');
mkdirSync(out, { recursive: true });

const body = '#f4a27c', belly = '#fde8dc', ear = '#f9c9b0', blush = '#ec8a66';
const tail = 'M96 98 C120 96 120 66 104 62';

const cat = `
<g transform="translate(20 40)">
  <path d="${tail}" fill="none" stroke="${body}" stroke-width="9" stroke-linecap="round"/>
  <polygon points="30,32 28,4 54,20" fill="${body}" stroke-linejoin="round"/>
  <polygon points="90,32 92,4 66,20" fill="${body}" stroke-linejoin="round"/>
  <polygon points="34,25 33,11 46,19" fill="${ear}"/>
  <polygon points="86,25 87,11 74,19" fill="${ear}"/>
  <ellipse cx="60" cy="88" rx="38" ry="29" fill="${body}"/>
  <ellipse cx="60" cy="94" rx="21" ry="17" fill="${belly}"/>
  <ellipse cx="44" cy="114" rx="10" ry="6" fill="${body}"/>
  <ellipse cx="76" cy="114" rx="10" ry="6" fill="${body}"/>
  <circle cx="60" cy="50" r="34" fill="${body}"/>
  <circle cx="38" cy="60" r="5" fill="${blush}" opacity="0.55"/>
  <circle cx="82" cy="60" r="5" fill="${blush}" opacity="0.55"/>
  <circle cx="47" cy="50" r="4.5" fill="#2a211c"/>
  <circle cx="73" cy="50" r="4.5" fill="#2a211c"/>
  <circle cx="48.6" cy="48.4" r="1.6" fill="#ffffff"/>
  <circle cx="74.6" cy="48.4" r="1.6" fill="#ffffff"/>
  <polygon points="56.5,58 63.5,58 60,62" fill="#d9775a"/>
  <path d="M60 62 Q57 66 53 64 M60 62 Q63 66 67 64" fill="none" stroke="#2a211c" stroke-width="2" stroke-linecap="round"/>
</g>`;

// The cat spans x 20..~143, y 44..160 in the 160 viewBox; centre it with a little margin.
const appSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="8 26 144 144">${cat}</svg>`;
writeFileSync(
  join(out, 'app-icon.png'),
  new Resvg(appSvg, { fitTo: { mode: 'width', value: 1024 }, background: 'rgba(0,0,0,0)' }).render().asPng(),
);

// Head silhouette: head circle cx60 cy50 r34 plus both ears. Bounds x 26..94, y 4..84 → 68×80.
const head = `
<g fill="#000000">
  <polygon points="30,32 28,4 54,20"/>
  <polygon points="90,32 92,4 66,20"/>
  <circle cx="60" cy="50" r="34"/>
</g>`;
const traySvg = `<svg xmlns="http://www.w3.org/2000/svg" width="44" height="44" viewBox="16 2 88 88">${head}</svg>`;
writeFileSync(
  join(out, 'tray-template.png'),
  new Resvg(traySvg, { fitTo: { mode: 'width', value: 44 }, background: 'rgba(0,0,0,0)' }).render().asPng(),
);
console.log('icons written to', out);
