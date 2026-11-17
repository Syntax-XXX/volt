// Volt im Browser: lädt app.wasm, ersetzt die WASI-Funktionen (Ausgabe → Konsole) und verbindet die Seite (DOM) mit dem Programm.
const dec = new TextDecoder(), enc = new TextEncoder();

export async function start(url) {
  const ex = {};
  let mem, out = '';
  const bytes = () => new Uint8Array(mem.buffer);
  const view = () => new DataView(mem.buffer);
  const str = (p) => { const u = bytes(); let e = p; while (u[e]) e++; return dec.decode(u.subarray(p, e)); };
  const alloc = (s) => { const b = enc.encode(s + '\0'); const p = ex.volt_alloc(b.length); bytes().set(b, p); return p; };
  const $ = (s) => document.querySelector(str(s));

  const wasi = {
    fd_write(fd, iovs, n, written) {
      let text = '', total = 0;
      for (let i = 0; i < n; i++) {
        const p = view().getUint32(iovs + i * 8, true), l = view().getUint32(iovs + i * 8 + 4, true);
        text += dec.decode(bytes().subarray(p, p + l)); total += l;
      }
      view().setUint32(written, total, true);
      out += text;
      const lines = out.split('\n'); out = lines.pop();
      for (const l of lines) (fd === 2 ? console.error : console.log)(l);
      return 0;
    },
    proc_exit(code) { throw new Error('Programm beendet (' + code + ')'); },
    environ_sizes_get(a, b) { view().setUint32(a, 0, true); view().setUint32(b, 0, true); return 0; },
    environ_get() { return 0; },
    args_sizes_get(a, b) { view().setUint32(a, 0, true); view().setUint32(b, 0, true); return 0; },
    args_get() { return 0; },
    clock_time_get(_id, _prec, p) { view().setBigUint64(p, BigInt(Date.now()) * 1000000n, true); return 0; },
    random_get(p, n) { crypto.getRandomValues(bytes().subarray(p, p + n)); return 0; },
    fd_close() { return 0; },
    fd_seek() { return 70; },
    fd_fdstat_get() { return 8; },
  };
  const volt = {
    set_text: (s, t) => { const e = $(s); if (e) e.textContent = str(t); },
    set_html: (s, h) => { const e = $(s); if (e) e.innerHTML = str(h); },
    append_html: (s, h) => { const e = $(s); if (e) e.insertAdjacentHTML('beforeend', str(h)); },
    get_value: (s) => { const e = $(s); return alloc(e && e.value != null ? String(e.value) : ''); },
    set_value: (s, v) => { const e = $(s); if (e) e.value = str(v); },
    listen: (s, ev, id) => { const e = $(s); if (e && ex.volt_event) e.addEventListener(str(ev), () => ex.volt_event(id)); },
    every: (ms, id) => { if (ex.volt_event) setInterval(() => ex.volt_event(id), ms); },
    log: (t) => console.log(str(t)),
  };
  const { instance } = await WebAssembly.instantiate(await (await fetch(url)).arrayBuffer(), { wasi_snapshot_preview1: wasi, volt });
  Object.assign(ex, instance.exports);
  mem = ex.memory;
  if (ex._initialize) ex._initialize();
  ex.volt_start();
}
