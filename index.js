// roadstyle for GeoLibre (docs/design/style_export.md, version 2): a roadstyle export (style.json + tiles + sprite, made by
// rs.export_style from a duckOSM GeoParquet file) as one layer over GeoLibre's own base map. The style is roadstyle's, made in
// Python; this plugin only adds it to GeoLibre's map and does the interaction: one row in the Layers panel, a popup with the
// road's data on click, the hover highlight. Open one with the menu "roadstyle" or the link parameter ?roadstyle=<style.json URL>.
// Or put roadstyle's simple look on a GeoLibre data layer: menu "roadstyle" > Style a layer with roadstyle (styleLayer, below).

const ID = "roadstyle";
const added = [];            // [{url, prefix, layerIds, sourceIds}]
let app = null;
let disposeMenu = null;
let popup = null;
let hover = null;            // {source, sourceLayer, id}

const VERSION = "0.19.1";            // the same as plugin.json (tests/test_plugin.py checks it)
const map = () => (app && app.getMap ? app.getMap() : null);
const esc = (v) => String(v).replace(/[&<>"']/g, (c) => ({"&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;"}[c]));

// ---- a small interface of the plugin's own (no prompt() or alert()): a dialog, a message, a progress box ----------------------
let uiReady = false;
function uiCss() {
  if (uiReady) return;
  uiReady = true;
  const st = document.createElement("style");
  st.textContent = `.rs-pl{color-scheme:light dark;font:14px/1.45 system-ui,"Segoe UI",sans-serif}
.rs-pl-overlay{position:fixed;inset:0;background:#0006;display:grid;place-items:center;z-index:2000}
.rs-pl-box{background:Canvas;color:CanvasText;border-radius:12px;padding:18px 20px;width:min(440px,92vw);box-shadow:0 8px 32px #0005}
.rs-pl-box h3{margin:0 0 12px;font-size:16px}
.rs-pl-box label{display:block;margin:10px 0 4px;font-weight:600;font-size:13px}
.rs-pl-box input,.rs-pl-box select{width:100%;box-sizing:border-box;font:inherit;padding:7px 9px;border-radius:8px;border:1px solid #8886;background:Field;color:FieldText}
.rs-pl-hint{margin:4px 0 0;color:GrayText;font-size:12px}
.rs-pl-btns{display:flex;justify-content:flex-end;gap:8px;margin-top:16px}
.rs-pl button{font:inherit;padding:7px 14px;border-radius:8px;border:1px solid #8886;background:ButtonFace;color:ButtonText;cursor:pointer}
.rs-pl button[type=submit]{background:#2563eb;border-color:#2563eb;color:#fff}
.rs-pl button:focus-visible,.rs-pl input:focus-visible,.rs-pl select:focus-visible{outline:2px solid #2563eb;outline-offset:2px}
.rs-pl-toast,.rs-pl-progress{position:fixed;left:50%;transform:translateX(-50%);z-index:2001;display:flex;gap:12px;align-items:center;
  background:Canvas;color:CanvasText;border-radius:10px;padding:10px 14px;box-shadow:0 4px 18px #0004;max-width:min(560px,92vw)}
.rs-pl-toast{bottom:18px;border-left:4px solid #dc2626}.rs-pl-toast.warn{border-left-color:#d97706}.rs-pl-toast.info{border-left-color:#2563eb}
.rs-pl-progress{top:14px}`;
  document.head.appendChild(st);
}
// a dialog: fields [{name, label, type, options, placeholder, required, hint}] -> {name: value}, or null when cancelled
function ask(title, fields, ok = "OK") {
  uiCss();
  return new Promise((done) => {
    const back = document.activeElement, ov = document.createElement("div"), form = document.createElement("form");
    ov.className = "rs-pl-overlay rs-pl";
    form.className = "rs-pl-box";
    form.setAttribute("role", "dialog"); form.setAttribute("aria-modal", "true"); form.setAttribute("aria-label", title);
    form.innerHTML = `<h3>${esc(title)}</h3>` + fields.map((f, i) => `<label for="rs-pl-${i}">${esc(f.label)}</label>` +
      (f.type === "select"
        ? `<select id="rs-pl-${i}">${f.options.map((o) => `<option value="${esc(o.value)}"${o.value === f.value ? " selected" : ""}>${esc(o.label)}</option>`).join("")}</select>`
        : `<input id="rs-pl-${i}" type="${f.type || "text"}" placeholder="${esc(f.placeholder || "")}"${f.accept ? ` accept="${esc(f.accept)}"` : ""}${f.required ? " required" : ""}>`) +
      (f.hint ? `<p class="rs-pl-hint">${esc(f.hint)}</p>` : "")).join("") +
      `<div class="rs-pl-btns"><button type="button" data-cancel>Cancel</button><button type="submit">${esc(ok)}</button></div>`;
    const close = (v) => { document.removeEventListener("keydown", onKey, true); ov.remove(); if (back && back.focus) back.focus(); done(v); };
    const onKey = (e) => { if (e.key === "Escape") { e.stopPropagation(); close(null); } };
    const val = (f, i) => { const el = form.querySelector("#rs-pl-" + i); return f.type === "file" ? (el.files[0] || null) : el.value.trim(); };
    form.onsubmit = (e) => { e.preventDefault(); close(Object.fromEntries(fields.map((f, i) => [f.name, val(f, i)]))); };
    form.querySelector("[data-cancel]").onclick = () => close(null);
    ov.onmousedown = (e) => { if (e.target === ov) close(null); };
    document.addEventListener("keydown", onKey, true);
    ov.appendChild(form); document.body.appendChild(ov);
    const first = form.querySelector("input,select"); if (first) first.focus();
  });
}
// a message that stays until it is clicked (an error) or for a while; role=alert so a screen reader says it
function notify(text, kind = "error") {
  uiCss();
  console[kind === "error" ? "error" : "info"]("roadstyle:", text);
  const t = document.createElement("div");
  t.className = "rs-pl rs-pl-toast " + kind; t.setAttribute("role", kind === "error" ? "alert" : "status");
  t.innerHTML = `<span>${esc(text)}</span><button type="button" aria-label="Close">\u2715</button>`;
  t.querySelector("button").onclick = () => t.remove();
  document.body.appendChild(t);
  if (kind !== "error") setTimeout(() => t.remove(), 9000);
}
// the progress box of a long job; onCancel makes a Cancel button. Returns {say(text), done()}.
function progress(text, onCancel) {
  uiCss();
  const b = document.createElement("div");
  b.className = "rs-pl rs-pl-progress"; b.setAttribute("role", "status"); b.setAttribute("aria-live", "polite");
  b.innerHTML = '<span></span>' + (onCancel ? '<button type="button">Cancel</button>' : "");
  if (onCancel) b.querySelector("button").onclick = onCancel;
  const span = b.querySelector("span"), say_ = (t) => { span.textContent = "roadstyle: " + t; console.info("roadstyle:", t); };
  say_(text); document.body.appendChild(b);
  return {say: say_, done: () => b.remove()};
}

// A map has one glyph source, the base map's: the names take a font the base map uses, not the export's own.
function baseFont(m) {
  for (const l of m.getStyle().layers) {
    const f = l.layout && l.layout["text-font"];
    if (Array.isArray(f) && f.every((x) => typeof x === "string")) return f;
  }
  return null;
}

// the export's sprite (the one-way arrow): each icon cut out of the PNG and added to the map under the layer's prefix
async function loadSprite(m, url, prefix) {
  if (!url) return [];
  const [index, png] = await Promise.all([fetch(url + ".json").then((r) => r.json()), fetch(url + ".png").then((r) => r.blob())]);
  const bitmap = await createImageBitmap(png);
  const canvas = document.createElement("canvas");
  canvas.width = bitmap.width; canvas.height = bitmap.height;
  const ctx = canvas.getContext("2d");
  ctx.drawImage(bitmap, 0, 0);
  for (const [name, d] of Object.entries(index)) {
    if (!m.hasImage(prefix + name)) m.addImage(prefix + name, ctx.getImageData(d.x, d.y, d.width, d.height), {pixelRatio: d.pixelRatio || 1, sdf: !!d.sdf});
  }
  return Object.keys(index).map((n) => prefix + n);
}

// the look's feature under the cursor: [styled entry, feature] of the top one, or null
function lookAt(m, point) {
  const ids = styled.flatMap((s) => s.ids).filter((id) => m.getLayer(id));
  const f = ids.length ? m.queryRenderedFeatures(point, {layers: ids})[0] : null;
  const s = f && styled.find((x) => x.ids.includes(f.layer.id));
  return s ? [s, f] : null;
}
let lookHover = null, lookTimer = 0;
function onLookMove(e) {
  const m = map();
  if (!m) return;
  const hit = lookAt(m, e.point);
  const key = hit && hit[0].hover ? styled.indexOf(hit[0]) + "|" + hit[1].properties[hit[0].hover.id] : null;
  if (!added.length) m.getCanvas().style.cursor = hit && hit[0].popup ? "pointer" : "";
  if (key === lookHover) return;
  lookHover = key;
  const light = (on) => {                    // the feature under the mouse in its look's hover layers, every other off
    for (const s of styled) {
      const v = on && hit[0] === s && s.hover ? String(hit[1].properties[s.hover.id]) : "\u0000";
      s.hoverIds.forEach((id) => { const f = m.getFilter(id); f[f.length - 1] = ["==", ["to-string", ["get", s.hover.id]], v]; m.setFilter(id, f); });
    }
  };
  light(false);                              // the last one off at once
  clearTimeout(lookTimer);
  if (hit) lookTimer = setTimeout(() => { if (lookHover === key) light(true); }, rsUI ? rsUI.inter.hover_delay_ms : 0);   // the page's rest
}
function onLookClick(e) {
  const m = map();
  if (!m || (app.getActiveMapTool && app.getActiveMapTool() !== null)) return;     // a GeoLibre tool (Identify, ...) has the click
  if (added.length && roadAt(m, e.point)) return;                                  // a roadstyle export's road has its own popup
  const hit = lookAt(m, e.point);
  if (!hit || !hit[0].popup) { if (!added.length) closePopup(); return; }
  const pp = hit[0].popup, p = hit[1].properties, no = ["", "nan", "none", "null", "no", "false", "0"];
  if (rsUI) {                                 // the page's popup (popup.js / popup.css), at the click
    closePopup();
    popup = makePopup(m, e.lngLat, rsUI.rfields(p, pp.columns), {close: true});
    return;
  }
  const shown = (k) => p[k] != null && !["", "nan", "none", "null"].includes(String(p[k]).toLowerCase()) &&
    !((pp.only_true || []).includes(k) && no.includes(String(p[k]).toLowerCase()));
  showPopup(m, e.point, Object.fromEntries(pp.columns.filter(shown).map((k) => [k, p[k]])), shown(pp.title) ? p[pp.title] : null);
}
function closePopup() {
  if (popup) popup.remove();
  popup = null;
}

// the popup in MapLibre's own popup look (its CSS classes): the plugin holds no style values
function showPopup(m, point, props, title = null) {
  closePopup();
  const rows = Object.entries(props).filter(([k]) => !k.startsWith("__")).map(([k, v]) => `<b>${esc(k)}</b>: ${esc(v)}`);
  popup = document.createElement("div");
  popup.className = "maplibregl-popup maplibregl-popup-anchor-top-left";
  popup.style.cssText = `position:absolute;left:0;top:0;transform:translate(${point.x}px,${point.y}px)`;
  popup.innerHTML = `<div class="maplibregl-popup-content"><button class="maplibregl-popup-close-button" type="button" aria-label="Close">×</button>` +
    (title != null ? `<b>${esc(title)}</b><br>` : "") + rows.join("<br>") + "</div>";
  popup.querySelector("button").onclick = closePopup;
  m.getContainer().appendChild(popup);
}

const ROAD_LIKE = new Set(["raceway", "other_ways"]);       // the groups of mapstyle's layers that are roads but in no road network: in the Roads folder, and selectable
// the road under the cursor: the roads tile layer of an added export (in simple mode its fill layer is unseen but kept for this)
function roadAt(m, point) {
  const ids = added.flatMap((a) => a.layerIds).filter((id) => m.getLayer(id) && m.getLayer(id).type === "line");
  const hits = m.queryRenderedFeatures(point, {layers: ids});
  // a road of the networks first; then a raceway or another way that no network holds (layers of their own, from mapstyle 0.9.0): they are roads too
  return hits.find((f) => f.sourceLayer === "roads" || (!f.sourceLayer && /roads$/.test(f.source)))
    || hits.find((f) => ROAD_LIKE.has(f.layer && f.layer.metadata && f.layer.metadata["roadstyle:group"])) || null;
}

// The page's road popup and hover (its popup.css / popup.js and its settings, style metadata roadstyle:interaction): the click
// selects a road (feature-state select) and opens its popup; resting the mouse on a road for the page's delay lights it up
// (feature-state hover) and shows its tooltip, when the page has one.
let rsUI = null;                     // {inter, rfields, POPUP}
let selected = null, tip = null, tipTimer = 0, tipKey = null;
async function loadUI(inter) {
  if (rsUI || !inter) return;
  inter = {css: asset("popup.css"), js: asset("popup.js"), ...inter};
  if (!document.querySelector('link[data-roadstyle="popup"]')) {
    const link = Object.assign(document.createElement("link"), {rel: "stylesheet", href: inter.css});
    link.dataset.roadstyle = "popup";
    document.head.appendChild(link);
  }
  const mod = await import(inter.js);
  // popup.js's _rfields puts the values into HTML as they are; the plugin builds the same lines from _rpairs with every name and value escaped
  const rfields = (p, only) => {
    const [name, kv] = mod._rpairs(p, only);
    return (name ? ["<b>" + esc(name) + "</b>"] : []).concat(kv.map(([k, v]) => "<b>" + esc(k) + "</b>: " + esc(v))).join("<br>");
  };
  rsUI = {inter, rfields, POPUP: mod.POPUP};
}
function fid(f) { return f && f.id != null ? {source: f.source, ...(f.sourceLayer ? {sourceLayer: f.sourceLayer} : {}), id: f.id} : null; }
function setState(m, k, st) { if (k) m.setFeatureState(k, st); }
// a popup in MapLibre's own markup (the page's popup.css styles it under .rs-plugin), kept at its place on the map
function makePopup(m, lngLat, html, opts) {
  const wrap = document.createElement("div");
  wrap.className = "rs-plugin";
  wrap.innerHTML = `<div class="maplibregl-popup maplibregl-popup-anchor-bottom ${opts.className || ""}" style="max-width:${rsUI.POPUP.maxWidth}">` +
    `<div class="maplibregl-popup-tip"></div><div class="maplibregl-popup-content">${html}` +
    (opts.close ? '<button class="maplibregl-popup-close-button" type="button" aria-label="Close popup">×</button>' : "") + "</div></div>";
  const el = wrap.firstChild;
  const place = () => { const p = m.project(pop.lngLat); el.style.transform = `translate(-50%,-100%) translate(${p.x}px,${p.y - (opts.offset || 0)}px)`; };
  const pop = {lngLat, el: wrap, setLngLat(ll) { pop.lngLat = ll; place(); return pop; },
               remove() { m.off("move", place); wrap.remove(); }};
  if (opts.close) el.querySelector("button").onclick = () => { pop.remove(); if (popup === pop) popup = null; };
  m.getContainer().appendChild(wrap);
  m.on("move", place); place();
  return pop;
}
function onClick(e) {
  const m = map();
  if (!m || (app.getActiveMapTool && app.getActiveMapTool() !== null)) return;     // a GeoLibre tool (Identify, ...) has the click
  const f = roadAt(m, e.point);
  setState(m, selected, {select: false});
  selected = fid(f);
  setState(m, selected, {select: true});
  closePopup();
  if (f) document.dispatchEvent(new CustomEvent("rs:select", {detail: {properties: f.properties}}));       // the route planner's panel keeps the edge_id of the road
  if (f && rsUI && rsUI.inter.popup !== false)
    popup = makePopup(m, e.lngLat, rsUI.rfields(f.properties, "highway" in f.properties ? rsUI.inter.popup : ["name", "kind"]), {close: true});
}
function onMove(e) {
  const m = map();
  if (!m) return;
  const f = roadAt(m, e.point), k = fid(f), key = k ? k.source + "|" + k.id : null;
  m.getCanvas().style.cursor = f ? "pointer" : "";
  if (key !== tipKey) {                                     // another road, or none: the last one off at once
    setState(m, hover, {hover: false}); hover = null;
    if (tip) { tip.remove(); tip = null; }
    clearTimeout(tipTimer); tipKey = key;
    if (!k) return;
    tipTimer = setTimeout(() => {                           // the mouse rested on it: light it up
      hover = k; setState(m, hover, {hover: true});
      const t = rsUI && rsUI.inter.tooltip;
      if (t) tip = makePopup(m, e.lngLat, rsUI.rfields(f.properties, Array.isArray(t) ? t : null), {className: "rs-tip", offset: rsUI.POPUP.tipOffset});
    }, rsUI ? rsUI.inter.hover_delay_ms : 0);
  } else if (tip) tip.setLngLat(e.lngLat);                  // lit: its tooltip follows the mouse along the road
}

async function addImages(m, images) {   // {name: data URL, or {url, sdf, pixelRatio}}
  await Promise.all(Object.entries(images || {}).map(([n, v]) => new Promise((done) => {
    const o = typeof v === "string" ? {url: v} : v, img = new Image();
    img.onload = () => { if (!m.hasImage(n)) m.addImage(n, img, {sdf: !!o.sdf, pixelRatio: o.pixelRatio || 1}); done(); };
    img.onerror = () => { console.warn("roadstyle: the image", n, "did not load"); done(); };
    img.src = o.url;
  })));
}

async function addStyle(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`roadstyle: ${url} answered ${r.status}`);
  return addStyleObject(await r.json(), url, "style");
}
// a roadstyle style (export_style's style.json, or export_inline's style with the data inside) as GeoLibre rows
// roadstyle's export, adjusted for GeoLibre's map (these two belong in roadstyle's export_style; remove them when it has them)
// The groups of a style (metadata roadstyle:group) and what the plugin does with them. Three parts in GeoLibre's Layers panel: Background (drawn
// under the roads), Roads, Features (over the roads). Two groups are left out unless the user chooses them in the dialog (opts.ocean, opts.decks):
// the ocean polygon (the blue sea of the mapstyle page: over GeoLibre's own map it looks like a blue land background) and the 3D bridge decks
// (roadstyle's export draws them as blocks at 0.7 opacity, and the water shows through). GeoLibre ignores the starting visibility of a row, so a
// row cannot start switched off: a group that starts off would show at once. Every other group, also those that the mapstyle page starts hidden
// (crossings, traffic signals), is shown, and the user switches it off in the panel. Sources that no layer uses any more go too.
const DECKS = "3D bridge decks", OPTIONAL = {ocean: "ocean", [DECKS]: "decks"};
const groupOf = (l) => (l.metadata && l.metadata["roadstyle:group"]) || "roads";
// roadstyle's flat road layers skip a bridge from the zoom where its 3D deck starts (["any", ["<", ["zoom"], 16], ["!", __rs_bridge]]).
// Without the decks nothing would draw the bridge there, so the bridges vanish when the user zooms in: the flat layers draw it at every zoom.
const isDeckSplit = (f) => Array.isArray(f) && f[0] === "any" && f.length === 3 && Array.isArray(f[1]) && f[1][0] === "<" && JSON.stringify(f[1][1]) === '["zoom"]'
  && JSON.stringify(f[2]) === '["!",["to-boolean",["get","__rs_bridge"]]]';
const flatBridges = (f) => isDeckSplit(f) ? true : Array.isArray(f) ? f.map(flatBridges) : f;
function arrange(style, opts) {
  for (const l of style.layers) if (l.source === "decks") l.metadata = {...(l.metadata || {}), "roadstyle:group": DECKS};   // a group of its own, not part of "roads"
  style.layers = style.layers.filter((l) => !(groupOf(l) in OPTIONAL) || opts[OPTIONAL[groupOf(l)]] === true);
  if (opts.decks !== true) for (const l of style.layers) if (l.filter) l.filter = flatBridges(l.filter);
  for (const l of style.layers) if (l.layout && l.layout.visibility === "none") l.layout.visibility = "visible";
  const used = new Set(style.layers.map((l) => l.source));
  for (const k of Object.keys(style.sources)) if (!used.has(k)) delete style.sources[k];
  return style;
}
function fixExport(style) {
  for (const l of style.layers)        // the 3D bridge decks are drawn at 0.7: the water shows through and the bridges look transparent
    if (l.type === "fill-extrusion" && l.paint && groupOf(l) === DECKS) l.paint["fill-extrusion-opacity"] = 1;     // only the decks: the 3D buildings keep mapstyle's 0.9
  return style;
}

// ---- road types: one GeoLibre row for each highway class of the roads, and for bridges and tunnels. A row's check box hides or shows
// those roads in every road layer with a filter, so the layers stay one set (a class is not a copy of them). A row has a transparent
// layer of its own, because GeoLibre calls a row's bridge (setVisibility) only for a row with a MapLibre layer id.
const CLASS_ORDER = ["motorway", "motorway_link", "trunk", "trunk_link", "primary", "primary_link", "secondary", "secondary_link",
  "tertiary", "tertiary_link", "unclassified", "residential", "living_street", "service", "pedestrian", "footway", "cycleway", "path",
  "steps", "track", "corridor", "platform", "ferry"];
function roadTypes(style) {            // the classes in the roads' own data; a style with tiles has none inline: the usual classes
  const src = style.sources.roads, feats = src && src.data && src.data.features;
  if (!feats) return {classes: CLASS_ORDER.slice(), bridge: true, tunnel: true};
  const have = new Set(feats.map((f) => f.properties && f.properties.highway).filter((c) => typeof c === "string" && c));
  const classes = [...have].sort((a, b) => (CLASS_ORDER.indexOf(a) < 0 ? 99 : CLASS_ORDER.indexOf(a)) - (CLASS_ORDER.indexOf(b) < 0 ? 99 : CLASS_ORDER.indexOf(b)) || a.localeCompare(b));
  return {classes, bridge: feats.some((f) => f.properties && f.properties.__rs_bridge), tunnel: feats.some((f) => f.properties && f.properties.__rs_tunnel)};
}
// The opacity of the roads: each road-type row has a slider, and the folder's slider sets every row. The factor of a road is the one of its class,
// of "bridges" (a bridge) and of "tunnels" (a tunnel), so a row fades its own roads. 1 for all puts the style's own values back.
function setRoadOpacity(m, a) {
  const op = a.op, f = [];
  if (op.classes.size) f.push(["match", ["coalesce", ["get", "highway"], ""], ...[...op.classes.values()].flat(), 1]);
  if (op.bridge < 1) f.push(["case", ["to-boolean", ["get", "__rs_bridge"]], op.bridge, 1]);
  if (op.tunnel < 1) f.push(["case", ["to-boolean", ["get", "__rs_tunnel"]], op.tunnel, 1]);
  setOpacity(m, a.roadLayers.map((x) => x.id), f.length ? (f.length === 1 ? f[0] : ["*", ...f]) : 1);
}
function setRoadFilters(m, a) {
  const h = a.hidden, conds = [];
  if (h.classes.size) conds.push(["!", ["in", ["coalesce", ["get", "highway"], ""], ["literal", [...h.classes]]]]);
  if (h.bridge) conds.push(["!", ["to-boolean", ["get", "__rs_bridge"]]]);
  if (h.tunnel) conds.push(["!", ["to-boolean", ["get", "__rs_tunnel"]]]);
  for (const {id, filter} of a.roadLayers)
    if (m.getLayer(id)) m.setFilter(id, conds.length ? ["all", ...(filter ? [filter] : []), ...conds] : (filter || null));
}

const IMAGE_PROPS = ["icon-image", "fill-pattern", "line-pattern", "fill-extrusion-pattern", "background-pattern"];
function renameImages(v, own, prefix) {          // in an expression: a string that is the name of one of the style's images
  if (typeof v === "string") return v in own ? prefix + v : v;
  return Array.isArray(v) ? v.map((x) => renameImages(x, own, prefix)) : v;
}
// The opacity of a row (and of a folder: GeoLibre's slider "Opacity" under a folder name): the opacity properties of its layers, times the value of
// the slider. The style's own values are kept, so the value 1 puts them back.
const OPACITY = {fill: ["fill-opacity"], line: ["line-opacity"], circle: ["circle-opacity", "circle-stroke-opacity"], symbol: ["icon-opacity", "text-opacity"],
  "fill-extrusion": ["fill-extrusion-opacity"], raster: ["raster-opacity"]};
const baseOpacity = new Map();
// A value times the opacity. MapLibre takes a zoom expression only at the top of the value (an interpolate or a step), so the factor goes into
// the outputs of its stops; for any other expression the whole expression is multiplied.
function scaleOpacity(v, o) {
  if (typeof v === "number") return typeof o === "number" ? v * o : ["*", o, v];
  if (Array.isArray(v) && v[0] === "interpolate" && Array.isArray(v[2]) && v[2][0] === "zoom")
    return v.map((x, i) => (i >= 4 && i % 2 === 0 ? scaleOpacity(x, o) : x));
  if (Array.isArray(v) && v[0] === "step" && Array.isArray(v[1]) && v[1][0] === "zoom")
    return v.map((x, i) => (i === 2 || (i >= 4 && i % 2 === 0) ? scaleOpacity(x, o) : x));
  return ["*", o, v];
}
function setOpacity(m, ids, o) {
  for (const id of ids) {
    const l = m.getLayer(id);
    if (!l) continue;
    for (const prop of OPACITY[l.type] || []) {
      const key = id + "|" + prop;
      if (!baseOpacity.has(key)) { const v = m.getPaintProperty(id, prop); baseOpacity.set(key, v == null ? 1 : v); }
      const b = baseOpacity.get(key);
      m.setPaintProperty(id, prop, o >= 1 ? b : scaleOpacity(b, o));
    }
  }
}

let prefixN = 0;                       // a prefix is never used twice, also after a map was removed
async function addStyleObject(style, url, kind = "style", opts = {}) {
  const m = map();
  if (!m) throw new Error("roadstyle: GeoLibre has no MapLibre map (the plugin needs the MapLibre renderer)");
  arrange(style, opts);
  fixExport(style);
  await loadUI(style.metadata && style.metadata["roadstyle:interaction"]);
  const prefix = `rs${prefixN++}-`;
  const spriteImages = style.sprite ? await loadSprite(m, style.sprite, prefix) : [];
  // the style's own images (roadstyle's arrow, mapstyle's icons and textures: roadstyle:images), added under their names
  // Under the map's prefix: GeoLibre's base map has an image named "oneway" of its own (21 x 21, a different arrow), and a name that
  // exists is not added again, so the arrows would be the base map's, not roadstyle's.
  const own = (style.metadata && style.metadata["roadstyle:images"]) || {};
  const images = Object.fromEntries(Object.entries(own).map(([n, v]) => [prefix + n, v]));
  await addImages(m, images);
  const font = baseFont(m);
  const sourceIds = [], layerIds = [];
  for (const [id, src] of Object.entries(style.sources)) {
    m.addSource(prefix + id, src);
    sourceIds.push(prefix + id);
  }
  for (const l of style.layers) {
    const layout = {...(l.layout || {})};
    if (layout["text-font"] && font) layout["text-font"] = font;
    const paint = l.paint ? {...l.paint} : undefined;
    for (const k of IMAGE_PROPS) {                                   // the image names of the layer, with the prefix (strings, and strings in expressions)
      if (typeof layout[k] === "string") layout[k] = prefix + layout[k];
      else if (k in layout) layout[k] = renameImages(layout[k], own, prefix);
      if (paint && k in paint) paint[k] = typeof paint[k] === "string" ? prefix + paint[k] : renameImages(paint[k], own, prefix);
    }
    m.addLayer({...l, id: prefix + l.id, source: prefix + l.source, layout, ...(paint ? {paint} : {})});
    layerIds.push(prefix + l.id);
  }
  // one GeoLibre row per group of the style (metadata roadstyle:group: "roads", or a base-map layer's name), in one folder,
  // so each can be switched on and off; a group starts as the style has it (a layer hidden in the page starts hidden)
  const groups = new Map();
  style.layers.forEach((l, i) => {
    const g = (l.metadata && l.metadata["roadstyle:group"]) || "roads";
    if (!groups.has(g)) groups.set(g, []);
    groups.get(g).push(layerIds[i]);
  });
  const roadsRows = [], featureRows = [], backgroundRows = [], typeRows = [], groupIds = [];
  const roadGroups = ["roads", DECKS];                  // the roads and their decks: the road types filter both
  const entry = {url, kind, prefix, layerIds, sourceIds, groupIds, imageIds: Object.keys(images).concat(spriteImages), hidden: {classes: new Set(), bridge: false, tunnel: false}, op: {classes: new Map(), bridge: 1, tunnel: 1},
                 roadLayers: style.layers.map((l, i) => ({id: layerIds[i], filter: l.filter || null, g: groupOf(l)}))
                   .filter((x) => roadGroups.includes(x.g))};
  if (app.registerExternalNativeLayer) {
    const roadsAt = [...groups.keys()].indexOf("roads");
    const registerTypes = () => {
    // the road types: a row for each class, bridges and tunnels (a row with a transparent layer of its own: its check box sets the filters)
    const types = roadTypes(style);
    // GeoLibre keeps the layers of its rows in the order of the panel, and leaves the layers that no row owns where they were (under the rows it moves).
    // There is no row for all the roads, so the first road-type row owns the road layers too: its check box only sets filters, so this changes nothing but their place.
    let anchor = groups.get("roads") || [];
    const typeRow = (key, name, hide, fade) => {
      const id = prefix + "type-" + key, own = anchor; anchor = [];
      let shown = true;                                  // GeoLibre calls the bridge once at the start: nothing to do then
      // GeoLibre treats a row as a plugin layer only when it has a MapLibre layer id: a transparent background layer is the row's own
      m.addLayer({id, type: "background", paint: {"background-opacity": 0}});
      layerIds.push(id);
      app.registerExternalNativeLayer({
        id, name, type: "vector", nativeLayerIds: [id, ...own], sourceIds: [], opacity: 1, style: {}, metadata: {url, group: "road type"}, visible: true,
        paintMode: "plugin",
        paintBridge: {setVisibility: (v) => {
          if (v === shown) return;
          shown = v; hide(!v); setRoadFilters(m, entry);
          console.info("roadstyle: road type", name, v ? "shown" : "hidden");
        }, setOpacity: (o) => { fade(o); setRoadOpacity(m, entry); }},
      });
      typeRows.push(id);
    };
    // GeoLibre lists the last row on top: bridges and tunnels, then the classes from motorway to path, so register them the other way round
    for (const c of types.classes.slice().reverse()) typeRow("c-" + c, c, (h) => entry.hidden.classes[h ? "add" : "delete"](c), (o) => { if (o >= 1) entry.op.classes.delete(c); else entry.op.classes.set(c, [c, o]); });
    if (types.tunnel) typeRow("tunnel", "tunnels", (h) => { entry.hidden.tunnel = h; }, (o) => { entry.op.tunnel = o; });
    if (types.bridge) typeRow("bridge", "bridges", (h) => { entry.hidden.bridge = h; }, (o) => { entry.op.bridge = o; });
    };

    [...groups].forEach(([g, ids], i) => {               // GeoLibre lists them with the last one on top, as they are drawn
      if (g === "roads") { registerTypes(); return; }    // the road types are the rows of the Roads folder, and there is no row for all roads: the folder switches them all
      const id = prefix + "row" + i, visible = ids.some((x) => m.getLayoutProperty(x, "visibility") !== "none");
      app.registerExternalNativeLayer({
        id, name: g, type: "vector", nativeLayerIds: ids, sourceIds,
        opacity: 1, style: {}, metadata: {url, group: g}, visible,
        paintMode: "plugin",          // roadstyle paints these layers: GeoLibre's style editors do not apply
        paintBridge: {setVisibility: (v) => ids.forEach((x) => m.getLayer(x) && m.setLayoutProperty(x, "visibility", v ? "visible" : "none")),
                      setOpacity: (o) => { setOpacity(m, ids, o); console.info("roadstyle: opacity", g, o); }},
      });
      // A GeoLibre folder is drawn at the place of its first row. So the layers that the style draws under the roads (ocean, land
      // cover, water, buildings, railways, parking) go to the folder Background, and the layers over the roads (crossings, signals,
      // stations, 3D buildings) to the folder Features. The decks are drawn over the roads, but they are part of the Roads folder.
      (roadGroups.includes(g) || ROAD_LIKE.has(g) ? roadsRows : i < roadsAt ? backgroundRows : featureRows).push(id);
    });
    if (app.addLayerGroup) {
      const name = style.name || "map", folder = (title, rows) => {
        if (!rows.length) return null;
        const g = app.addLayerGroup(`${title} (${name})`, rows);
        groupIds.push(g);
        return g;
      };
      // No folder inside a folder: GeoLibre moves a row of the outer folder into the next one when a folder is nested (tested).
      folder("Background", backgroundRows);
      folder("Roads", typeRows.concat(roadsRows));
      folder("Features", featureRows);
    }
  }
  if (!added.length) { m.on("click", onClick); m.on("mousemove", onMove); }   // the popup stays at its road while the map moves, as on the page
  entry.rowIds = typeRows.concat(roadsRows, backgroundRows, featureRows);
  added.push(entry);
  if (style.center && style.zoom != null) m.jumpTo({center: style.center, zoom: style.zoom});
  const small = style.layers.filter((l) => l.minzoom >= 16 && m.getLayoutProperty(prefix + l.id, "visibility") !== "none");
  if (small.length && m.getZoom() < 16) notify("Crossings, traffic signals and other small features show from zoom 16: zoom in to see them.", "info");
  const meta = style.metadata || {};
  if (meta["roadstyle:planner"]) await setupPlanner(entry, meta["roadstyle:planner"], kind);
  else if (meta["roadstyle:planner_skipped"]) notify("The database has " + meta["roadstyle:planner_skipped"] + " roads: too many for the route planner in the browser.", "warn");
}

// ---- the route planner: the panel of `duckosm route-map` (docs/design/routing.md). planner/planner.js and its panel are made from duckOSM's template by
// tools/make_planner.py, so the plugin shows the same panel. The data (mapstyle's planner_data) is in the style's metadata roadstyle:planner.
// A draggable marker with the methods of MapLibre's (the host's MapLibre class is not reachable from a plugin): a DOM element over the map.
class PlannerMarker {
  constructor(map, {element}) { this.map = map; this.el = element; this.ll = null; this.fn = {}; this.el.style.position = "absolute"; this.el.style.left = "0"; this.el.style.top = "0"; this.el.style.zIndex = "10"; this.el.style.pointerEvents = "auto"; this.el.style.touchAction = "none"; }
  on(ev, f) { (this.fn[ev] = this.fn[ev] || []).push(f); return this; }
  fire(ev) { (this.fn[ev] || []).forEach((f) => f()); }
  getLngLat() { return this.ll; }
  setLngLat(ll) { this.ll = Array.isArray(ll) ? {lng: ll[0], lat: ll[1]} : {lng: ll.lng, lat: ll.lat}; this.place(); return this; }
  place() { if (!this.ll) return; const p = this.map.project([this.ll.lng, this.ll.lat]), w = this.el.offsetWidth || 30, h = this.el.offsetHeight || 38;
    this.el.style.transform = `translate(${p.x - w / 2}px,${p.y - h}px)`; }      // anchor: bottom (the tip of the pin)
  addTo() {
    this.map.getContainer().appendChild(this.el);
    this.move = () => this.place(); for (const ev of ["move", "resize"]) this.map.on(ev, this.move);       // a resize (the side panel opens) moves the map without a move event
    let drag = false;
    this.el.addEventListener("pointerdown", (e) => { drag = true; this.el.setPointerCapture(e.pointerId); e.preventDefault(); e.stopPropagation(); });
    this.el.addEventListener("pointermove", (e) => {
      if (!drag) return;
      const r = this.map.getContainer().getBoundingClientRect(), h = this.el.offsetHeight || 38;
      const ll = this.map.unproject([e.clientX - r.left, e.clientY - r.top]);     // the tip of the pin is at the pointer
      this.ll = {lng: ll.lng, lat: ll.lat}; this.place(); this.fire("drag");
    });
    const end = (e) => { if (!drag) return; drag = false; try { this.el.releasePointerCapture(e.pointerId); } catch (x) { /* released */ } this.fire("dragend"); };
    this.el.addEventListener("pointerup", end); this.el.addEventListener("pointercancel", end);
    this.place(); return this;
  }
  remove() { if (this.move) for (const ev of ["move", "resize"]) this.map.off(ev, this.move); this.el.remove(); }
}
let planner = null;                       // {id, dispose}: the one route planner on the map
function disposePlanner() {
  if (!planner) return;
  try { planner.mounted && planner.mounted.dispose(); } catch (e) { console.warn("roadstyle: the route planner did not clean up:", e); }
  if (planner.unregister) planner.unregister();
  planner.holder.remove();
  planner = null;
}
async function setupPlanner(entry, RM, kind) {
  const m = map();
  disposePlanner();
  if (!document.querySelector('link[data-roadstyle="planner"]')) {
    const link = Object.assign(document.createElement("link"), {rel: "stylesheet", href: asset("planner/panel.css")});
    link.dataset.roadstyle = "planner";
    document.head.appendChild(link);
  }
  const html = await (await fetch(asset("planner/panel.html"))).text(), mod = await import(asset("planner/planner.js"));
  const src = () => m.getSource(entry.prefix + "roads"), feats = () => { const d = src()._data; return (d.geojson || d).features; };
  const H = {roadsData: () => src()._data, roadIds: () => feats().map((f, i) => i), roadProps: (ids) => ids.map((i) => (feats()[i] || {}).properties || {}),
             recolor: () => {}, Marker: class extends PlannerMarker { constructor(o) { super(m, o); } }};
  // the panel lives in a hidden holder, so its ids are in the document while the code mounts; the right panel moves it in and out
  const holder = document.createElement("div"), root = document.createElement("div");
  holder.style.display = "none"; root.className = "rs-planner"; root.innerHTML = html;
  holder.appendChild(root); document.body.appendChild(holder);
  planner = {id: "roadstyle-planner", holder, entry, open: () => {}};
  try { planner.mounted = mod.mount(m, RM, H); }
  catch (e) { console.error("roadstyle: the route planner did not start:", e); const msg = String(e && e.message || e); disposePlanner(); return notify("The route planner could not start: " + msg, "error"); }
  entry.planner = true;
  // where the panel goes: GeoLibre's right panel; else a floating panel; else a box of our own over the map (an older GeoLibre has neither call)
  const panel = {id: planner.id, title: "Route planner", defaultWidth: 380, render: (container) => { container.appendChild(root); return () => { holder.appendChild(root); }; }};
  let how = null;
  if (app.registerRightPanel && app.openRightPanel) {
    try { planner.unregister = app.registerRightPanel({...panel, dock: "right-of-style"}); planner.open = () => app.openRightPanel(panel.id); how = "right panel"; }
    catch (e) { console.warn("roadstyle: the right panel was refused:", e); }
  }
  if (!how && app.registerFloatingPanel && app.openFloatingPanel) {
    try { planner.unregister = app.registerFloatingPanel({...panel, position: "top-right"}); planner.open = () => app.openFloatingPanel(panel.id); how = "floating panel"; }
    catch (e) { console.warn("roadstyle: the floating panel was refused:", e); }
  }
  if (!how) {
    const box = document.createElement("div"), bar = document.createElement("div");
    box.style.cssText = "position:fixed;top:60px;right:12px;bottom:12px;width:380px;max-width:92vw;z-index:1500;overflow:auto;background:Canvas;color:CanvasText;border-radius:12px;box-shadow:0 8px 30px #0005";
    bar.style.cssText = "position:sticky;top:0;text-align:right;padding:4px 8px;z-index:1;background:Canvas";
    bar.innerHTML = '<button type="button" aria-label="Close" style="border:0;background:none;font-size:20px;cursor:pointer;color:inherit">\u00d7</button>';
    bar.firstChild.onclick = () => { box.hidden = true; };
    box.append(bar, root); document.body.appendChild(box);
    planner.unregister = () => box.remove(); planner.open = () => { box.hidden = false; }; how = "box over the map";
  }
  console.info("roadstyle: the route planner is in the " + how);
  planner.open();
  // the panel narrows the map: after it has opened, the map shows the route from the start to the end (the page opens on the whole area, where the route is)
  const [a, b] = [RM.start, RM.end];
  setTimeout(() => { try { m.resize(); m.fitBounds([[Math.min(a[0], b[0]), Math.min(a[1], b[1])], [Math.max(a[0], b[0]), Math.max(a[1], b[1])]], {padding: 90, duration: 0}); } catch (e) { /* the map is gone */ } }, 600);
}
function openPlanner() {
  if (!planner) return notify("Open a duckOSM database with the route planner on first (roadstyle > Open a duckOSM database).", "warn");
  planner.open();
}

// ---- approach 2 (docs/design/style_export.md): roadstyle's simple look on a GeoLibre data layer (DuckDB, GeoParquet, GeoJSON with
// duckOSM columns). The look (rs.export_look: layers with no source, made in Python from a profile and a palette) goes on the data
// layer's own MapLibre source; GeoLibre's plain line goes unseen but stays, so Identify still finds the roads.
const styled = [];           // [{layer, ids}]

// look: a roadstyle look (rs.export_look) or one file of a mapstyle look (mapstyle.look.export_look); fetched from lookUrl if not given
async function styleLayer(layerId, lookUrl, look = null, label = "roadstyle look") {
  const m = map();
  const src = "source-" + layerId;
  if (!m || !m.getSource(src)) throw new Error("roadstyle: this layer has no data source the plugin can read (" + src + ")");
  look = look || await (await fetch(lookUrl || asset("looks/default.json"))).json();
  const feats = (app.getLayerFeatures ? app.getLayerFeatures(layerId) : null);
  const props = ((feats && feats.features) || feats || []).map((f) => f.properties || {});
  const missing = (look.columns || []).filter((c) => !props.some((p) => c in p));
  // the positions the data has (a road without them is at position 0): only their layers are added
  const have = new Set(props.flatMap((p) => [p.casing_level, p.fill_level]).map((v) => (v == null ? 0 : Number(v))));
  const all = m.getStyle().layers, ids = all.map((l) => l.id);
  const own = all.filter((l) => l.source === src);                  // GeoLibre's own layers of this data layer
  const before = ids[ids.indexOf(own[own.length - 1].id) + 1];      // right above them
  const added_ = [];
  for (const l of look.layers) {
    const pos = l.metadata && l.metadata["roadstyle:position"];
    if (pos != null && !have.has(pos)) continue;
    const id = "rslook-" + layerId + "-" + l.id;
    m.addLayer({...l, id, source: src}, before);
    added_.push(id);
  }
  const NONE = "\u0000";                                             // matches no feature: nothing is hovered
  const hoverIds = ((look.hover && look.hover.layers) || []).map((l) => {
    const id = "rslook-" + layerId + "-" + l.id;
    m.addLayer({...l, id, source: src, filter: ["all", ...(l.filter ? [l.filter] : []), ["==", ["to-string", ["get", look.hover.id]], NONE]]}, before);
    return id;
  });
  ownColor[layerId] = Object.fromEntries(own.map((l) => [l.type, m.getPaintProperty(l.id, l.type + "-color")])
    .filter(([, c]) => typeof c === "string"));
  hideOwn(layerId);
  if (app.registerExternalNativeLayer) {
    app.registerExternalNativeLayer({
      id: "rslook-" + layerId, name: ((app.listLayers().find((l) => l.id === layerId) || {}).name || layerId) + " · " + label, type: "vector",
      nativeLayerIds: added_.concat(hoverIds), sourceIds: [], opacity: 1, style: {}, paintMode: "plugin", metadata: {layerId, lookUrl},
      paintBridge: {setVisibility: (v) => added_.concat(hoverIds).forEach((id) => m.getLayer(id) && m.setLayoutProperty(id, "visibility", v ? "visible" : "none"))},
    });
  }
  styled.push({layer: layerId, ids: added_, hoverIds, hover: look.hover, popup: look.popup, lookUrl});
  if (styled.length === 1) { m.on("mousemove", onLookMove); m.on("click", onLookClick); m.on("movestart", closePopup); }
  if (missing.length) notify("The layer has no column " + missing.join(", ") + ": the look shows what it can" +
                             (missing.includes("fill_level") ? " (every road at one drawing position)." : "."), "warn");
}

// GeoLibre's own line of the data layer, unseen (a transparent colour), set through GeoLibre's style so that GeoLibre keeps it: it
// redraws its layers from its own store, and a paint set on the map alone would come back. GeoLibre's layer style has a line colour but
// no line opacity. The layer stays, so Identify still finds the roads. ``color``: the colour to put back (unstyleAll).
const ownColor = {};
// GeoLibre's own drawing of the layer: transparent under a look, its colours again without it (importLayerStyle, so
// GeoLibre's layer sync keeps it and Identify still works)
function hideOwn(layerId, colors = null) {
  const types = ["fill", "line", "circle"];
  const res = app.importLayerStyle ? app.importLayerStyle(layerId, JSON.stringify({version: 8, sources: {}, layers: types.map((t) =>
    ({id: t, type: t, source: "d", paint: {[t + "-color"]: (colors && colors[t]) || "rgba(0,0,0,0)"}}))})) : null;
  console.info("roadstyle: GeoLibre's own drawing of layer", layerId, "->", colors ? "its colours" : "transparent", JSON.stringify(res));
}

// the dialog of "Style a layer with roadstyle": which data layer, and optionally another look (empty: the plugin's default look)
async function askStyleLayer() {
  const layers = (app.listLayers ? app.listLayers() : []).filter((l) => l.type !== "raster");
  if (!layers.length) return notify("Add a data layer first (for example the edges of a duckOSM file).", "warn");
  const r = await ask("Style a layer with roadstyle", [
    {name: "layer", label: "Layer", type: "select", options: layers.map((l) => ({value: l.id, label: l.name || l.id}))},
    {name: "look", label: "Look (optional)", type: "url", placeholder: "the plugin's default look",
     hint: "The URL of a look from rs.export_look. Leave empty for the default look."}], "Style");
  if (r) await styleLayer(r.layer, r.look || null);
}

function unstyleAll() {
  const m = map();
  for (const s of styled.splice(0)) {
    if (app.unregisterExternalNativeLayer) app.unregisterExternalNativeLayer("rslook-" + s.layer);
    if (!m) continue;
    s.ids.concat(s.hoverIds || []).forEach((id) => m.getLayer(id) && m.removeLayer(id));
    hideOwn(s.layer, ownColor[s.layer] || {});
  }
  if (m) { m.off("mousemove", onLookMove); m.off("click", onLookClick); if (!added.length) m.off("movestart", closePopup); }
}

function removeEntry(a, m) {
  if (a.planner) disposePlanner();
  if (app.unregisterExternalNativeLayer) (a.rowIds || []).forEach((id) => app.unregisterExternalNativeLayer(id));
  if (app.removeLayerGroup) (a.groupIds || []).slice().reverse().forEach((g) => g && app.removeLayerGroup(g));
  if (!m) return;
  a.layerIds.slice().reverse().forEach((id) => m.getLayer(id) && m.removeLayer(id));
  a.sourceIds.forEach((id) => m.getSource(id) && m.removeSource(id));
  (a.imageIds || []).forEach((id) => m.hasImage(id) && m.removeImage(id));
}
function removeAll() {
  const m = map();
  closePopup();
  for (const a of added.splice(0)) removeEntry(a, m);
  if (m) { m.off("click", onClick); m.off("mousemove", onMove); }
}
function removeKind(kind) {            // the maps of one kind (a database's map before it is drawn again)
  const m = map();
  closePopup();
  for (let i = added.length - 1; i >= 0; i--) if (added[i].kind === kind) removeEntry(added.splice(i, 1)[0], m);
  if (m && !added.length) { m.off("click", onClick); m.off("mousemove", onMove); }
}

const fail = (e) => notify(String((e && e.message) || e));

// the plugin's own files (looks, py/worker.js, py/pkgs.zip, popup.css / popup.js): by their URL when GeoLibre gives one (a plugin installed by its
// manifest URL); a plugin installed from a .zip has none, so the zip's index.js carries them (write_plugin_zip fills ASSETS)
const ASSETS = /*__ASSETS__*/null;
const TYPES = {js: "text/javascript", css: "text/css", html: "text/html", json: "application/json", zip: "application/zip"};
const blobs = {};                                  // a file of the .zip as a blob URL, made once
function asset(f) {
  const url = app.resolvePluginAssetUrl && app.resolvePluginAssetUrl(ID, f);
  if (url) return url;
  if (ASSETS && f in ASSETS) {
    if (!blobs[f]) {
      const ext = f.split(".").pop(), body = ext === "zip" ? Uint8Array.from(atob(ASSETS[f]), (c) => c.charCodeAt(0)) : ASSETS[f];   // a zip as base64
      blobs[f] = URL.createObjectURL(new Blob([body], {type: TYPES[ext]}));
    }
    return blobs[f];
  }
  throw new Error("roadstyle: the plugin has no file " + f + " (install it by its manifest URL, or from the .zip that write_plugin_zip makes)");
}

// A duckOSM database from your disk: roadstyle and mapstyle make its map in the browser with their own Python code (Pyodide in a
// worker, py/worker.js; no server, no export), so it is the map of their pages; the plugin adds the style it makes as rows.
let pyWorker = null, pyReady = false, working = null;      // working: {cancel} while a map is being made
function stopWorker() {
  if (pyWorker) pyWorker.terminate();
  pyWorker = null; pyReady = false;
}
// one map at a time; Cancel stops the worker (the next map starts a new one)
function makeStyle(file, box, opts) {
  return new Promise(async (done, fail_) => {
    working = {cancel: () => { stopWorker(); fail_(new Error("cancelled")); }};      // Cancel works from the first moment
    try {
      const first = !pyWorker;
      if (first)                                     // a worker from a blob: GeoLibre's page cannot start one from another address
        pyWorker = new Worker(URL.createObjectURL(new Blob([await (await fetch(asset("py/worker.js"))).text()], {type: "text/javascript"})), {type: "module"});
      const w = pyWorker, msg = {db: await file.arrayBuffer(), name: file.name.replace(/\.duckdb$/i, ""),
                                 opts: {mode: opts.mode, theme: opts.theme, paths: opts.paths, planner: opts.planner !== "off"}};
      if (!pyReady) msg.pkgs = await (await fetch(asset("py/pkgs.zip"))).arrayBuffer();     // until the worker said it is ready
      w.onmessage = ({data}) => {
        if (data.step) box.say(data.step);
        else if (data.ready) pyReady = true;
        else if (data.error) { if (!pyReady) stopWorker(); fail_(new Error("the map of " + file.name + " failed: " + data.error)); }
        else done(data.style);
      };
      w.onerror = (e) => { stopWorker(); fail_(new Error("the Python worker failed: " + (e.message || "it could not start"))); };
      w.postMessage(msg, [msg.db, ...(msg.pkgs ? [msg.pkgs] : [])]);
    } catch (e) { stopWorker(); fail_(e); }
  });
}

// the colours of a database's map: mapstyle's modes, themes and path styles (the same as its page and command line)
const pick = (name, label, values, value) => ({name, label, type: "select", value, options: values.map(([v, l]) => ({value: v, label: l}))});
const dbFields = (file, o) => [
  ...(file ? [{name: "file", label: "duckOSM database (.duckdb)", type: "file", accept: ".duckdb", required: true}] : []),
  pick("mode", "Roads in front", [["all", "All modes"], ["driving", "Driving"], ["walking", "Walking"], ["cycling", "Cycling"]], o.mode),
  pick("theme", "Colours", [["osm", "OpenStreetMap"], ["google", "Google"], ["grey", "Grey"], ["roadstyle", "roadstyle default"]], o.theme),
  pick("paths", "Footpaths and cycleways", [["google", "Google (calm lines)"], ["osm", "OpenStreetMap (dotted)"], ["komoot", "Komoot (bold)"], ["cyclosm", "CyclOSM (blue)"]], o.paths),
  pick("ocean", "Ocean polygon (the blue sea)", [["off", "Off: GeoLibre's own map shows the sea"], ["on", "On, as the mapstyle page"]], o.ocean),
  pick("decks", "3D bridge decks", [["off", "Off"], ["on", "On"]], o.decks),
  pick("planner", "Route planner", [["on", "On: drop a start and an end on the map"], ["off", "Off"]], o.planner),
];
let lastDb = null;                                   // {file, opts}: "Redraw with other colours" makes the same file again
const DB_DEFAULTS = {mode: "all", theme: "osm", paths: "google", ocean: "off", decks: "off", planner: "on"};     // all: the crossings and signals show from their zoom; the page hides them

async function runDatabase(file, opts) {
  const box = progress("reading " + file.name, () => working && working.cancel());
  try {
    const style = await makeStyle(file, box, opts);
    box.say("drawing");
    removeKind("database");                          // the map of the last time goes when the new one is ready
    await addStyleObject(JSON.parse(style), file.name, "database", {ocean: opts.ocean === "on", decks: opts.decks === "on"});
    lastDb = {file, opts};
  } catch (e) {
    if (!(e && e.message === "cancelled")) fail(e);
  } finally {
    working = null; box.done();
  }
}

async function openDatabase() {
  if (working) return notify("A map is being made: wait for it, or cancel it.", "warn");
  const r = await ask("Open a duckOSM database", dbFields(true, lastDb ? lastDb.opts : DB_DEFAULTS), "Open");
  if (r) await runDatabase(r.file, r);
}

async function redrawDatabase() {
  if (working) return notify("A map is being made: wait for it, or cancel it.", "warn");
  if (!lastDb) return notify("Open a duckOSM database first.", "warn");
  const r = await ask("Redraw " + lastDb.file.name + " with other colours", dbFields(false, lastDb.opts), "Redraw");
  if (r) await runDatabase(lastDb.file, r);
}

async function askAddStyle() {
  const r = await ask("Add a roadstyle layer", [{name: "url", label: "URL of a roadstyle export (style.json)", type: "url", required: true,
    placeholder: "https://example.org/export/style.json"}], "Add");
  if (r) await addStyle(r.url);
}

function removeEverything() {
  removeAll();
  unstyleAll();
}

export const plugin = {
  id: ID,
  name: "roadstyle",
  version: VERSION,
  urlParameterNames: ["roadstyle"],
  activate(host) {
    app = host;
    if (app.registerToolbarMenu) {
      const run = (f) => () => { Promise.resolve().then(f).catch(fail); };
      disposeMenu = app.registerToolbarMenu({
        id: "roadstyle-menu", label: "roadstyle",
        items: [{id: "db", label: "Open a duckOSM database\u2026", onSelect: run(openDatabase)},
                {id: "redraw", label: "Redraw the database with other colours\u2026", onSelect: run(redrawDatabase)},
                {id: "add", label: "Add a roadstyle layer\u2026", onSelect: run(askAddStyle)},
                {id: "look", label: "Style a layer with roadstyle\u2026", onSelect: run(askStyleLayer)},
                {id: "planner", label: "Route planner", onSelect: run(openPlanner)},
                {id: "remove", label: "Remove roadstyle layers", onSelect: run(removeEverything)}],
      });
    }
  },
  async handleUrlParameters(host, params) {
    app = app || host;
    for (const url of params.getAll("roadstyle")) await addStyle(url).catch(fail);
  },
  deactivate() {
    if (working) working.cancel();
    removeAll();
    unstyleAll();
    stopWorker();
    if (disposeMenu) disposeMenu();
    disposeMenu = null;
  },
  getProjectState() {
    return {urls: added.filter((a) => a.kind === "style").map((a) => a.url)};     // a database's map has no address to open again
  },
  applyProjectState(host, state) {
    app = app || host;
    if (!state || !Array.isArray(state.urls)) return false;
    state.urls.filter((u) => typeof u === "string" && /^(https?:|blob:|\/|\.)/.test(u) && !added.some((a) => a.url === u))
      .forEach((u) => addStyle(u).catch(fail));
  },
};

export const __test = {addStyleObject};     // for tests/test_plugin.py

export default plugin;
