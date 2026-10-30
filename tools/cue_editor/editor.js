// Cue/lyric timeline editor. Edits `set.direction.cues` of one packet: each cue's `t` and
// `word_times`. Everything else about a cue (palette, behavior, look, layout, priority, label)
// is round-tripped untouched.
//
// Two pieces of math here mirror the Rust renderer exactly, on purpose, not as a style choice:
//   snapGhost()   mirrors crates/direct/src/scenes.rs's snap(): most cue boundaries silently
//                 jump onto the nearest kick/snare onset within snap_window_s. Measured against
//                 this song, that's already moved 4 of 16 cues by up to 210ms. A drag preview
//                 that doesn't show this is a display the renderer disagrees with.
//   projectedCut()  mirrors crates/direct/src/lyric.rs's lines(): a subline with no word_times
//                 still gets a real, computed cut (beat-grid-snapped from drop_t), not "no
//                 timing". Shown as a dashed preview so it's never silently invisible.

const qs = new URLSearchParams(location.search);
const PACKET = qs.get('packet') || 'hopeless';

const el = {
  playBtn: document.getElementById('playBtn'),
  timeReadout: document.getElementById('timeReadout'),
  zoom: document.getElementById('zoom'),
  hashWarning: document.getElementById('hashWarning'),
  saveBtn: document.getElementById('saveBtn'),
  status: document.getElementById('status'),
  scroller: document.getElementById('scroller'),
  track: document.getElementById('track'),
  ruler: document.getElementById('ruler'),
  waveform: document.getElementById('waveform'),
  cueLayer: document.getElementById('cueLayer'),
  wordLayer: document.getElementById('wordLayer'),
  playhead: document.getElementById('playhead'),
  audio: document.getElementById('audio'),
};

let S = null;       // server state (cues + song facts), mutated in place as the working copy
let pxPerSec = 80;
let dirty = false;
let selected = -1;
let linkWordTimes = true;
document.getElementById('linkWordTimes').addEventListener('change', (ev) => { linkWordTimes = ev.target.checked; });

function setStatus(msg, isErr) {
  el.status.textContent = msg;
  el.status.classList.toggle('err', !!isErr);
}

function setDirty(v) {
  dirty = v;
  el.saveBtn.classList.toggle('dirty', v);
}

async function load() {
  const res = await fetch(`/api/state?packet=${encodeURIComponent(PACKET)}`);
  const body = await res.json();
  if (!res.ok) { setStatus(`load failed: ${body.error}`, true); throw new Error(body.error); }
  S = body;
  el.hashWarning.hidden = S.audio_hash_ok;
  el.audio.src = `/api/audio?packet=${encodeURIComponent(PACKET)}`;
  document.title = `cue editor — ${PACKET}`;
  render();
}

// ---- domain helpers: mirror the Rust side exactly, nothing stricter, nothing invented ----

function cueWords(text) {
  return (text || '').split(' / ').flatMap(sub => sub.split(/\s+/).filter(Boolean));
}

function sublineCounts(text) {
  return (text || '').split(' / ').map(sub => sub.split(/\s+/).filter(Boolean).length);
}

function cuePriority(cue) {
  return cue.priority ?? S.default_cue_priority;
}

function isSnapExempt(cue) {
  return cuePriority(cue) === S.snap_exempt_priority;
}

// Mirrors scenes.rs::snap(): nearest onset among snap_onsets within snap_window_s, else null.
function snapGhost(t) {
  let best = null, bestD = Infinity;
  for (const [ot, stem] of S.snap_onsets) {
    const d = Math.abs(ot - t);
    if (d <= S.snap_window_s && d < bestD) { best = { t: ot, stem }; bestD = d; }
  }
  return best;
}

// The real boundary scenes.rs will use for cues[i], given its *currently authored* t.
function effectiveStart(i) {
  const c = S.cues[i];
  if (isSnapExempt(c)) return c.t;
  const g = snapGhost(c.t);
  return g ? g.t : c.t;
}

function effectiveEnd(i) {
  return i + 1 < S.cues.length ? effectiveStart(i + 1) : S.duration_s;
}

// Mirrors lyric.rs::lines()'s beat-grid guess for an untimed subline's cut, clamped the same way.
function projectedCuts(t0, t1, counts) {
  const n = counts.length;
  const beat = 60.0 / S.bpm;
  const minS = S.lyric_min_s;
  const cuts = [t0];
  for (let k = 1; k < n; k++) {
    const lo = cuts[k - 1] + minS;
    const hi = t1 - minS * (n - k);
    const guess = t0 + (t1 - t0) * k / n;
    const raw = S.drop_t + Math.round((guess - S.drop_t) / beat) * beat;
    cuts.push(lo <= hi ? Math.min(Math.max(raw, lo), hi) : raw);
  }
  cuts.push(t1);
  return cuts;
}

// ---- layout ----

function totalWidth() { return S.duration_s * pxPerSec; }
function xOf(t) { return t * pxPerSec; }
function tOf(x) { return x / pxPerSec; }

function render() {
  const w = totalWidth();
  el.track.style.width = `${w}px`;
  for (const c of [el.ruler, el.waveform]) { c.width = w; }
  el.ruler.height = 22;
  el.waveform.height = 70;
  el.cueLayer.setAttribute('width', w);
  el.wordLayer.setAttribute('width', w);
  el.cueLayer.setAttribute('viewBox', `0 0 ${w} 48`);
  el.wordLayer.setAttribute('viewBox', `0 0 ${w} 90`);

  drawRuler();
  drawWaveform();
  drawCues();
  drawWords();
}

function drawRuler() {
  const ctx = el.ruler.getContext('2d');
  ctx.clearRect(0, 0, el.ruler.width, 22);
  ctx.fillStyle = '#1b1b1b'; ctx.fillRect(0, 0, el.ruler.width, 22);
  ctx.strokeStyle = '#444'; ctx.fillStyle = '#888'; ctx.font = '10px monospace';
  const step = pxPerSec < 40 ? 5 : pxPerSec < 100 ? 1 : 0.5;
  for (let t = 0; t <= S.duration_s; t += step) {
    const x = xOf(t);
    ctx.beginPath(); ctx.moveTo(x, 12); ctx.lineTo(x, 22); ctx.stroke();
    if (Math.round(t / step) % (step < 1 ? 4 : 1) === 0) ctx.fillText(t.toFixed(step < 1 ? 1 : 0), x + 2, 10);
  }
  const dx = xOf(S.drop_t);
  ctx.strokeStyle = '#e66'; ctx.beginPath(); ctx.moveTo(dx, 0); ctx.lineTo(dx, 22); ctx.stroke();
  ctx.fillStyle = '#e66'; ctx.fillText('drop', dx + 2, 20);
}

function drawWaveform() {
  const ctx = el.waveform.getContext('2d');
  const W = el.waveform.width, H = 70;
  ctx.clearRect(0, 0, W, H);
  ctx.fillStyle = '#181818'; ctx.fillRect(0, 0, W, H);
  const peaks = S.waveform.peaks, cols = S.waveform.columns_per_second;
  ctx.strokeStyle = '#5a8'; ctx.beginPath();
  for (let x = 0; x < W; x++) {
    const t = tOf(x);
    const i = Math.floor(t * cols);
    const p = Math.min(1, peaks[i] ?? 0);
    const h = p * (H - 4);
    ctx.moveTo(x + 0.5, H / 2 - h / 2);
    ctx.lineTo(x + 0.5, H / 2 + h / 2);
  }
  ctx.stroke();
  const dx = xOf(S.drop_t);
  ctx.strokeStyle = '#e66'; ctx.beginPath(); ctx.moveTo(dx, 0); ctx.lineTo(dx, H); ctx.stroke();
}

const NS = 'http://www.w3.org/2000/svg';
function svgEl(tag, attrs) {
  const e = document.createElementNS(NS, tag);
  for (const k in attrs) e.setAttribute(k, attrs[k]);
  return e;
}

function drawCues() {
  el.cueLayer.textContent = '';
  S.cues.forEach((cue, i) => {
    const x0 = xOf(cue.t);
    const x1 = xOf(i + 1 < S.cues.length ? S.cues[i + 1].t : S.duration_s);
    const block = svgEl('rect', { x: x0, y: 2, width: Math.max(1, x1 - x0), height: 30, class: 'cue-block' + (i === selected ? ' selected' : '') });
    block.dataset.index = i;
    block.addEventListener('pointerdown', onCueBlockDown);
    el.cueLayer.appendChild(block);

    const label = svgEl('text', { x: x0 + 4, y: 14, class: 'cue-label' });
    label.textContent = `${cue.label || `cue ${i}`}${cue.word_times ? '' : ' (uniform)'}`;
    el.cueLayer.appendChild(label);

    // 10px, not a hairline: a 4px target was unreliable to grab with a mouse or trackpad,
    // especially zoomed out.
    const handle = svgEl('rect', {
      x: x0 - 5, y: 0, width: 10, height: 34,
      class: 'cue-handle' + (isSnapExempt(cue) ? ' exempt' : ''),
    });
    handle.dataset.index = i;
    handle.addEventListener('pointerdown', onCueHandleDown);
    el.cueLayer.appendChild(handle);
  });
  renderCueToolbar();
}

function renderCueToolbar() {
  if (selected < 0) return;
  const cue = S.cues[selected];
  const x0 = xOf(cue.t);
  const label = svgEl('text', { x: x0 + 4, y: 44, class: 'cue-toolbar-text' });
  label.textContent = cue.word_times ? 'remove word timing' : 'add word timing';
  label.addEventListener('click', () => toggleWordTiming(selected));
  el.cueLayer.appendChild(label);
}

function toggleWordTiming(i) {
  const cue = S.cues[i];
  if (cue.word_times) {
    delete cue.word_times;
  } else {
    const t0 = effectiveStart(i), t1 = effectiveEnd(i);
    const n = cueWords(cue.text).length;
    const times = [];
    for (let k = 0; k < n; k++) times.push(round3(t0 + (t1 - t0) * k / Math.max(1, n)));
    cue.word_times = times;
  }
  setDirty(true);
  render();
}

function round3(x) { return Math.round(x * 1000) / 1000; }

function drawWords() {
  el.wordLayer.textContent = '';
  S.cues.forEach((cue, i) => {
    const t0 = effectiveStart(i), t1 = effectiveEnd(i);
    const counts = sublineCounts(cue.text);
    const n = counts.length;

    if (cue.word_times) {
      // Real per-word onsets: draggable, grouped visually by subline.
      let wi = 0;
      counts.forEach((count) => {
        for (let k = 0; k < count; k++, wi++) {
          const idx = wi;
          const t = cue.word_times[idx];
          const word = cueWords(cue.text)[idx];
          drawWordMark(t, word, idx, cue);
        }
      });
      // subline separators at each subline's first word
      let acc = 0;
      counts.forEach((count) => {
        if (acc > 0) drawSublineSep(cue.word_times[acc]);
        acc += count;
      });
    } else {
      // No real timing: show the beat-snapped projection lines()/`lyric.rs` will actually use.
      const cuts = projectedCuts(t0, t1, counts);
      for (let k = 1; k < n; k++) drawProjectedCut(cuts[k]);
    }
  });
}

function drawSublineSep(t) {
  const x = xOf(t);
  el.wordLayer.appendChild(svgEl('line', { x1: x, y1: 0, x2: x, y2: 90, class: 'subline-sep' }));
}

function drawProjectedCut(t) {
  const x = xOf(t);
  el.wordLayer.appendChild(svgEl('line', { x1: x, y1: 0, x2: x, y2: 90, class: 'proj-mark' }));
  const lbl = svgEl('text', { x: x + 2, y: 86, class: 'proj-text' });
  lbl.textContent = 'projected cut';
  el.wordLayer.appendChild(lbl);
}

function drawWordMark(t, word, wordIndex, cue) {
  const x = xOf(t);
  // Same reasoning as the cue handle: a wider hit area than the visible mark, so a word at high
  // zoom-out isn't a precision-only target.
  const mark = svgEl('rect', { x: x - 4, y: 4, width: 8, height: 14, class: 'word-mark' });
  mark.dataset.cueIndex = S.cues.indexOf(cue);
  mark.dataset.wordIndex = wordIndex;
  mark.addEventListener('pointerdown', onWordMarkDown);
  el.wordLayer.appendChild(mark);
  const lbl = svgEl('text', { x: x + 3, y: 16, class: 'word-text' });
  lbl.textContent = word;
  el.wordLayer.appendChild(lbl);
}

// ---- dragging ----
//
// Two distinct gestures, like trimming vs moving a clip in any real NLE/DAW:
//   - the edge handle (`cue-handle`) resizes: it changes only this cue's `t`, snap-previewed,
//     for setting exactly where a cut lands.
//   - the block body (`cue-block`) moves: it's a rigid-group drag of `t` and, when "move word
//     times with cue" is on, every one of this cue's word_times by the same delta, so shifting a
//     section doesn't orphan every word inside it. No snap preview here -- that's the handle's
//     job; the body is for coarse by-ear realignment of a whole line.
// A body-drag that never crosses a few pixels is treated as a plain click (select the cue),
// not a move, the same threshold every drag-capable UI uses to tell a click from a drag.

const CLICK_THRESHOLD_PX = 3;
let drag = null;

function onCueHandleDown(ev) {
  const i = Number(ev.currentTarget.dataset.index);
  ev.currentTarget.classList.add('dragging');
  drag = { kind: 'resize', index: i };
  window.addEventListener('pointermove', onPointerMove);
  window.addEventListener('pointerup', onPointerUp, { once: true });
}

function onCueBlockDown(ev) {
  const i = Number(ev.currentTarget.dataset.index);
  const cue = S.cues[i];
  drag = {
    kind: 'move', index: i, moved: false,
    startClientX: ev.clientX,
    startCueT: cue.t,
    startWordTimes: cue.word_times ? cue.word_times.slice() : null,
  };
  window.addEventListener('pointermove', onPointerMove);
  window.addEventListener('pointerup', onPointerUp, { once: true });
}

function onWordMarkDown(ev) {
  const cueIndex = Number(ev.currentTarget.dataset.cueIndex);
  const wordIndex = Number(ev.currentTarget.dataset.wordIndex);
  ev.currentTarget.classList.add('dragging');
  drag = { kind: 'word', cueIndex, wordIndex };
  window.addEventListener('pointermove', onPointerMove);
  window.addEventListener('pointerup', onPointerUp, { once: true });
}

function clientXToTrackT(clientX) {
  const rect = el.track.getBoundingClientRect();
  return tOf(clientX - rect.left);
}

function onPointerMove(ev) {
  if (!drag) return;
  if (drag.kind === 'resize') resizeCue(drag.index, clientXToTrackT(ev.clientX));
  else if (drag.kind === 'word') dragWord(drag.cueIndex, drag.wordIndex, clientXToTrackT(ev.clientX));
  else if (drag.kind === 'move') moveCueBody(ev);
}

function onPointerUp() {
  window.removeEventListener('pointermove', onPointerMove);
  document.querySelectorAll('.dragging').forEach(n => n.classList.remove('dragging'));
  if (drag && drag.kind === 'move' && !drag.moved) { selected = drag.index; }
  drag = null;
  render();
}

// Edge-handle: change just this cue's t, previewed through the same snap the renderer applies.
function resizeCue(i, rawT) {
  const cue = S.cues[i];
  const lo = i > 0 ? S.cues[i - 1].t + 0.01 : 0.0;
  const hi = i + 1 < S.cues.length ? S.cues[i + 1].t - 0.01 : S.duration_s - 0.01;
  let t = Math.min(Math.max(rawT, lo), hi);
  if (!isSnapExempt(cue)) {
    const g = snapGhost(t);
    if (g) t = Math.min(Math.max(g.t, lo), hi);
  }
  cue.t = round3(t);
  setDirty(true);
  render();
}

// Block body: rigid-shift t and (if linked) every word_time by the same delta. No snap preview --
// that's what the handle is for; this is coarse realignment of a whole section by ear.
function moveCueBody(ev) {
  const dxPx = ev.clientX - drag.startClientX;
  if (!drag.moved && Math.abs(dxPx) < CLICK_THRESHOLD_PX) return;
  drag.moved = true;

  const i = drag.index;
  const lo = i > 0 ? S.cues[i - 1].t + 0.01 : 0.0;
  const hi = i + 1 < S.cues.length ? S.cues[i + 1].t - 0.01 : S.duration_s - 0.01;
  let delta = Math.min(Math.max(drag.startCueT + dxPx / pxPerSec, lo), hi) - drag.startCueT;

  const wt = drag.startWordTimes;
  if (linkWordTimes && wt && wt.length) {
    const endBound = effectiveEnd(i) - 0.01; // next cue's real start; unaffected by this cue's move
    delta = Math.min(delta, endBound - wt[wt.length - 1]);
    delta = Math.max(delta, 0.0 - wt[0]);
    S.cues[i].word_times = wt.map(t => round3(t + delta));
  }
  S.cues[i].t = round3(drag.startCueT + delta);
  selected = i;
  setDirty(true);
  render();
}

function dragWord(cueIndex, wordIndex, rawT) {
  const cue = S.cues[cueIndex];
  const wt = cue.word_times;
  const lo = wordIndex > 0 ? wt[wordIndex - 1] : effectiveStart(cueIndex);
  const hi = wordIndex + 1 < wt.length ? wt[wordIndex + 1] : effectiveEnd(cueIndex);
  wt[wordIndex] = round3(Math.min(Math.max(rawT, lo), hi));
  setDirty(true);
  render();
}

// ---- playback ----

function fmtTime(t) { return t.toFixed(2); }

function updatePlayhead() {
  const t = el.audio.currentTime;
  el.playhead.style.left = `${xOf(t)}px`;
  el.timeReadout.textContent = `${fmtTime(t)} / ${fmtTime(S.duration_s)}`;
  if (!el.audio.paused) requestAnimationFrame(updatePlayhead);
}

el.playBtn.addEventListener('click', () => {
  if (el.audio.paused) { el.audio.play(); } else { el.audio.pause(); }
});
el.audio.addEventListener('play', () => { el.playBtn.textContent = '⏸'; updatePlayhead(); });
el.audio.addEventListener('pause', () => { el.playBtn.textContent = '▶'; updatePlayhead(); });

document.addEventListener('keydown', (ev) => {
  if (ev.code === 'Space' && ev.target === document.body) { ev.preventDefault(); el.playBtn.click(); }
});

[el.ruler, el.waveform].forEach(c => c.addEventListener('click', (ev) => {
  const rect = el.track.getBoundingClientRect();
  el.audio.currentTime = tOf(ev.clientX - rect.left);
  updatePlayhead();
}));

el.zoom.addEventListener('input', () => { pxPerSec = Number(el.zoom.value); render(); updatePlayhead(); });

// ---- save ----

el.saveBtn.addEventListener('click', async () => {
  setStatus('saving...');
  const res = await fetch(`/api/save?packet=${encodeURIComponent(PACKET)}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ cues: S.cues }),
  });
  const body = await res.json();
  if (!res.ok) { setStatus(`save rejected: ${body.error}`, true); return; }
  setDirty(false);
  setStatus('saved');
});

load().catch(err => setStatus(String(err), true));
