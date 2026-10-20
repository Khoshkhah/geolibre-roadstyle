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

const map = () => (app && app.getMap ? app.getMap() : null);

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
  if (!url) return;
  const [index, png] = await Promise.all([fetch(url + ".json").then((r) => r.json()), fetch(url + ".png").then((r) => r.blob())]);
  const bitmap = await createImageBitmap(png);
  const canvas = document.createElement("canvas");
  canvas.width = bitmap.width; canvas.height = bitmap.height;
  const ctx = canvas.getContext("2d");
  ctx.drawImage(bitmap, 0, 0);
  for (const [name, d] of Object.entries(index)) {
    if (!m.hasImage(prefix + name)) m.addImage(prefix + name, ctx.getImageData(d.x, d.y, d.width, d.height), {pixelRatio: d.pixelRatio || 1, sdf: !!d.sdf});
  }
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
  const esc = (v) => String(v).replace(/[&<>"]/g, (c) => ({"&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;"}[c]));
  const rows = Object.entries(props).filter(([k]) => !k.startsWith("__")).map(([k, v]) => `<b>${esc(k)}</b>: ${esc(v)}`);
  popup = document.createElement("div");
  popup.className = "maplibregl-popup maplibregl-popup-anchor-top-left";
  popup.style.cssText = `position:absolute;left:0;top:0;transform:translate(${point.x}px,${point.y}px)`;
  popup.innerHTML = `<div class="maplibregl-popup-content"><button class="maplibregl-popup-close-button" type="button" aria-label="Close">×</button>` +
    (title != null ? `<b>${esc(title)}</b><br>` : "") + rows.join("<br>") + "</div>";
  popup.querySelector("button").onclick = closePopup;
  m.getContainer().appendChild(popup);
}

// the road under the cursor: the roads tile layer of an added export (in simple mode its fill layer is unseen but kept for this)
function roadAt(m, point) {
  const ids = added.flatMap((a) => a.layerIds).filter((id) => m.getLayer(id) && m.getLayer(id).type === "line");
  return m.queryRenderedFeatures(point, {layers: ids}).find((f) => f.sourceLayer === "roads") || null;
}

// The page's road popup and hover (its popup.css / popup.js and its settings, style metadata roadstyle:interaction): the click
// selects a road (feature-state select) and opens its popup; resting the mouse on a road for the page's delay lights it up
// (feature-state hover) and shows its tooltip, when the page has one.
let rsUI = null;                     // {inter, rfields, POPUP}
let selected = null, tip = null, tipTimer = 0, tipKey = null;
async function loadUI(inter) {
  if (rsUI || !inter) return;
  if (!document.querySelector('link[data-roadstyle="popup"]')) {
    const link = Object.assign(document.createElement("link"), {rel: "stylesheet", href: inter.css});
    link.dataset.roadstyle = "popup";
    document.head.appendChild(link);
  }
  const mod = await import(inter.js);
  rsUI = {inter, rfields: mod._rfields, POPUP: mod.POPUP};
}
function fid(f) { return f && f.id != null ? {source: f.source, sourceLayer: "roads", id: f.id} : null; }
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
  if (f && rsUI && rsUI.inter.popup !== false)
    popup = makePopup(m, e.lngLat, rsUI.rfields(f.properties, rsUI.inter.popup), {close: true});
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

async function addStyle(url) {
  const m = map();
  if (!m) throw new Error("roadstyle: GeoLibre has no MapLibre map (the plugin needs the MapLibre renderer)");
  const style = await (await fetch(url)).json();
  await loadUI(style.metadata && style.metadata["roadstyle:interaction"]);
  const prefix = `rs${added.length}-`;
  await loadSprite(m, style.sprite, prefix);
  // the style's own images (mapstyle: icons and textures, roadstyle:images), added under their names
  const images = (style.metadata && style.metadata["roadstyle:images"]) || {};
  await Promise.all(Object.entries(images).map(([n, src]) => new Promise((done) => {
    const img = new Image();
    img.onload = () => { if (!m.hasImage(n)) m.addImage(n, img); done(); };
    img.onerror = () => { console.warn("roadstyle: the image", n, "did not load"); done(); };
    img.src = src;
  })));
  const font = baseFont(m);
  const sourceIds = [], layerIds = [];
  for (const [id, src] of Object.entries(style.sources)) {
    m.addSource(prefix + id, src);
    sourceIds.push(prefix + id);
  }
  for (const l of style.layers) {
    const layout = {...(l.layout || {})};
    if (layout["text-font"] && font) layout["text-font"] = font;
    if (typeof layout["icon-image"] === "string" && !(layout["icon-image"] in images)) layout["icon-image"] = prefix + layout["icon-image"];
    m.addLayer({...l, id: prefix + l.id, source: prefix + l.source, layout});
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
  const rowIds = [];
  if (app.registerExternalNativeLayer) {
    [...groups].forEach(([g, ids], i) => {               // GeoLibre lists them with the last one on top, as they are drawn
      const id = prefix + "row" + i, visible = ids.some((x) => m.getLayoutProperty(x, "visibility") !== "none");
      app.registerExternalNativeLayer({
        id, name: g === "roads" ? (style.name || "roads") + " · roads" : g, type: "vector", nativeLayerIds: ids, sourceIds,
        opacity: 1, style: {}, metadata: {url, group: g}, visible,
        paintMode: "plugin",          // roadstyle paints these layers: GeoLibre's style editors do not apply
        paintBridge: {setVisibility: (v) => ids.forEach((x) => m.getLayer(x) && m.setLayoutProperty(x, "visibility", v ? "visible" : "none"))},
      });
      rowIds.push(id);
    });
    if (app.addLayerGroup) app.addLayerGroup((style.name || "roads") + " (roadstyle)", rowIds);
  }
  if (!added.length) { m.on("click", onClick); m.on("mousemove", onMove); }   // the popup stays at its road while the map moves, as on the page
  added.push({url, prefix, layerIds, sourceIds, rowIds});
  if (style.center && style.zoom != null) m.jumpTo({center: style.center, zoom: style.zoom});
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
  look = look || await (await fetch(lookUrl)).json();
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
  if (missing.length) alert("roadstyle: the layer has no column " + missing.join(", ") + ": the look shows what it can" +
                            (missing.includes("fill_level") ? " (every road at one drawing position)." : "."));
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

function pickLayer() {
  const layers = (app.listLayers ? app.listLayers() : []).filter((l) => l.type !== "raster");
  if (!layers.length) { alert("roadstyle: add a data layer first (for example the edges of a duckOSM file)."); return null; }
  const choice = layers.length === 1 ? "1" : prompt("Which layer?\n" + layers.map((l, i) => (i + 1) + ": " + l.name).join("\n"), "1");
  const l = layers[Number(choice) - 1];
  return l ? l.id : null;
}

function unstyleAll() {
  const m = map();
  for (const s of styled.splice(0)) {
    if (s.own === false) continue;                  // a database's layers: closeDatabases removes them
    if (app.unregisterExternalNativeLayer) app.unregisterExternalNativeLayer("rslook-" + s.layer);
    if (!m) continue;
    s.ids.concat(s.hoverIds || []).forEach((id) => m.getLayer(id) && m.removeLayer(id));
    hideOwn(s.layer, ownColor[s.layer] || {});
  }
  if (m) { m.off("mousemove", onLookMove); m.off("click", onLookClick); if (!added.length) m.off("movestart", closePopup); }
}

function removeAll() {
  const m = map();
  closePopup();
  for (const a of added.splice(0)) {
    if (app.unregisterExternalNativeLayer) (a.rowIds || []).forEach((id) => app.unregisterExternalNativeLayer(id));
    if (!m) continue;
    a.layerIds.slice().reverse().forEach((id) => m.getLayer(id) && m.removeLayer(id));
    a.sourceIds.forEach((id) => m.getSource(id) && m.removeSource(id));
  }
  if (m) { m.off("click", onClick); m.off("mousemove", onMove); }
}

const fail = (e) => { console.error(e); alert(String(e.message || e)); };

// A duckOSM database from your disk, read in the browser (duckosm.js, DuckDB-wasm): no server and no export. The roads get
// roadstyle's look (looks/default.json, with their casing cut into heads at the file's drawing positions), the base map
// mapstyle's (looks/mapstyle.json), the boundary roadstyle's outline: one GeoLibre row per layer, in one folder.
const opened = [];                   // [{sources, layers, rows}]
function openDatabase() {
  const input = Object.assign(document.createElement("input"), {type: "file", accept: ".duckdb"});
  input.onchange = () => (async () => {
    const file = input.files && input.files[0];
    if (!file) return;
    const say = (step) => console.info("roadstyle:", file.name, "-", step);
    const [roadLook, mapLook] = await Promise.all([defaultLook(), defaultMapLook()].map(async (u) => (await fetch(u)).json()));
    await loadUI({css: asset("popup.css"), js: asset("popup.js"), popup: roadLook.popup.columns, tooltip: false, hover_delay_ms: roadLook.hover_delay_ms});
    // GeoLibre runs this file from a blob, so a relative import does not resolve: duckosm.js by its asset URL
    const {readDuckOSM, casingPieces} = await import(asset("duckosm.js"));
    const data = await readDuckOSM(file, say);
    say("drawing");
    drawDuckOSM(data, file.name, roadLook, mapLook, casingPieces);
    if (!data.levels) alert("roadstyle: " + file.name + " has no drawing order (visualization.edge_levels), so every road is drawn at one " +
                            "position: bridges and tunnels are not over or under other roads. Store it in the file with: duckosm levels " + file.name);
  })().catch(fail);
  input.click();
}

function drawDuckOSM(data, name, roadLook, mapLook, casingPieces) {
  const m = map(), k = `rd${opened.length}-`;
  const sources = [], layers = [], groups = new Map(), NONE = "\u0000";
  const src = (id, d) => { m.addSource(k + id, {type: "geojson", data: d}); sources.push(k + id); return k + id; };
  const add = (group, spec, source) => {
    const id = k + spec.id;
    m.addLayer({...spec, id, source});
    layers.push(id);
    if (group) { if (!groups.has(group)) groups.set(group, []); groups.get(group).push(id); }
    return id;
  };
  // a look's hover layers: they show only the feature under the mouse (onLookMove sets the id the last filter keeps)
  const hoverOf = (look, source, tag) => ((look.hover && look.hover.layers) || []).map((l) => add(null,
    {...l, id: `${tag}-hover-${l.id}`, filter: ["all", ...(l.filter ? [l.filter] : []), ["==", ["to-string", ["get", look.hover.id]], NONE]]}, source));
  const entries = [];
  for (const t of mapLook.order) {                   // the base map, in mapstyle's draw order
    const look = mapLook.files["features_" + t], d = data.features[t];
    if (!look || !d) continue;
    const s = src("f-" + t, d);
    const ids = look.layers.map((l) => add(l.id.replace(/-(outline|pattern)$/, ""), {...l, id: `f-${t}-${l.id}`}, s));
    entries.push({ids, hoverIds: hoverOf(look, s, "f-" + t), hover: look.hover, popup: look.popup});
  }
  if (data.boundary && roadLook.boundary)
    roadLook.boundary.layers.forEach((l) => add("boundary", {...l, id: "b-" + l.id}, src("b", data.boundary)));
  const rs = src("roads", data.roads), pieces = casingPieces(data.roads, roadLook.head_m), ps = src("pieces", pieces);
  const have = new Set([...pieces.features.map((f) => f.properties.casing_level), ...data.roads.features.map((f) => f.properties.fill_level ?? 0)]);
  const roadIds = [];
  for (const l of roadLook.layers) {
    const pos = l.metadata && l.metadata["roadstyle:position"];
    if (pos != null && !have.has(pos)) continue;
    roadIds.push(add("roads", {...l, id: "r-" + l.id}, l.id.startsWith("casing") ? ps : rs));
  }
  entries.push({ids: roadIds, hoverIds: hoverOf(roadLook, rs, "r"), hover: roadLook.hover, popup: roadLook.popup});
  const rows = [];
  if (app.registerExternalNativeLayer) {
    [...groups].forEach(([g, ids], i) => {           // in draw order: GeoLibre lists the last one on top
      const id = k + "row" + i;
      app.registerExternalNativeLayer({
        id, name: g === "roads" ? name + " · roads" : g, type: "vector", nativeLayerIds: ids, sourceIds: sources, opacity: 1, style: {},
        metadata: {file: name, group: g}, paintMode: "plugin",
        paintBridge: {setVisibility: (v) => ids.forEach((x) => m.getLayer(x) && m.setLayoutProperty(x, "visibility", v ? "visible" : "none"))},
      });
      rows.push(id);
    });
    if (app.addLayerGroup) app.addLayerGroup(name + " (roadstyle)", rows);
  }
  for (const e of entries) styled.push({layer: k, own: false, ...e});
  if (styled.length === entries.length) { m.on("mousemove", onLookMove); m.on("click", onLookClick); }
  opened.push({sources, layers, rows});
  let w = 180, s_ = 90, e_ = -180, n = -90;          // fit the map to the roads
  for (const f of data.roads.features) for (const [x, y] of f.geometry.coordinates) { w = Math.min(w, x); e_ = Math.max(e_, x); s_ = Math.min(s_, y); n = Math.max(n, y); }
  if (w <= e_) m.fitBounds([[w, s_], [e_, n]], {padding: 20, duration: 0});
}

function closeDatabases() {
  const m = map();
  for (const o of opened.splice(0)) {
    if (app.unregisterExternalNativeLayer) o.rows.forEach((id) => app.unregisterExternalNativeLayer(id));
    if (!m) continue;
    o.layers.slice().reverse().forEach((id) => m.getLayer(id) && m.removeLayer(id));
    o.sources.forEach((id) => m.getSource(id) && m.removeSource(id));
  }
}

// A duckOSM folder (duckosm export-gis --format geoparquet): GeoLibre opens many GeoParquet files from one link
// (?data=A&data=B), so the plugin makes that link from layers.json and adds roadstyle_look=<look URL>. After the
// page opens it again, the plugin styles every edges layer with the look.
const ORDER = {boundary: 0, features: 1, edges: 2}, MODES = {walking: 0, cycling: 1, driving: 2};
// the plugin's own files (looks, duckosm.js, popup.css / popup.js): by their URL when GeoLibre gives one (a plugin installed by its
// manifest URL); a plugin installed from a .zip has none, so the zip's index.js carries them (write_plugin_zip fills ASSETS)
const ASSETS = /*__ASSETS__*/null;
const TYPES = {js: "text/javascript", css: "text/css", json: "application/json"};
function asset(f) {
  const url = app.resolvePluginAssetUrl && app.resolvePluginAssetUrl(ID, f);
  if (url) return url;
  if (ASSETS && f in ASSETS) return URL.createObjectURL(new Blob([ASSETS[f]], {type: TYPES[f.split(".").pop()]}));
  throw new Error("roadstyle: the plugin has no file " + f + " (install it by its manifest URL, or from the .zip that write_plugin_zip makes)");
}
const defaultLook = () => asset("looks/default.json"), defaultMapLook = () => asset("looks/mapstyle.json");

async function openFolder(folder, lookUrl, mapLookUrl) {
  const base = folder.replace(/layers\.json$/, "").replace(/\/?$/, "/");
  const res = await fetch(base + "layers.json");
  if (!res.ok) throw new Error("roadstyle: no layers.json in " + base + " (HTTP " + res.status + "). Write the folder with: duckosm export-gis --format geoparquet");
  const mapLook = await (await fetch(mapLookUrl)).json(), tables = mapLook.order;   // the base map's draw order, from mapstyle
  // not opened: the nodes, and a features table that mapstyle does not draw (place_labels)
  const files = (await res.json()).layers.filter((f) => f.kind !== "nodes" && (f.kind !== "features" || mapLook.files[f.file.replace(/\.parquet$/, "")])).sort((a, b) =>
    (ORDER[a.kind] ?? 9) - (ORDER[b.kind] ?? 9) || (MODES[a.mode] ?? 9) - (MODES[b.mode] ?? 9) ||
    tables.indexOf(a.table) - tables.indexOf(b.table));
  const q = new URLSearchParams();
  files.forEach((f) => q.append("data", new URL(f.file, base).href));
  q.set("roadstyle_look", lookUrl);
  q.set("mapstyle_look", mapLookUrl);
  location.assign(location.origin + location.pathname + "?" + q.toString());
}

// Wait for GeoLibre to load the files of the link that pick(name) gives a look, then style each one.
async function styleFiles(pick, label) {
  const want = new URLSearchParams(location.search).getAll("data")
    .map((u) => decodeURIComponent(new URL(u).pathname.split("/").pop()).replace(/\.(geo)?parquet$/i, ""))
    .filter((n) => pick(n));
  const t0 = Date.now();
  for (;;) {
    const have = (app.listLayers ? app.listLayers() : []).filter((l) => want.includes(l.name));
    if (have.length === want.length && have.every((l) => map() && map().getSource("source-" + l.id))) {
      for (const l of have) await styleLayer(l.id, null, pick(l.name), label);
      return;
    }
    if (Date.now() - t0 > 600000) throw new Error("roadstyle: GeoLibre did not load " + want.join(", ") + " in 10 minutes; the look is not applied.");
    await new Promise((r) => setTimeout(r, 1000));
  }
}

export const plugin = {
  id: ID,
  name: "roadstyle",
  version: "0.9.0",
  urlParameterNames: ["roadstyle", "roadstyle_look", "mapstyle_look"],
  activate(host) {
    app = host;
    if (app.registerToolbarMenu) {
      disposeMenu = app.registerToolbarMenu({
        id: "roadstyle-menu", label: "roadstyle",
        items: [{id: "db", label: "Open a duckOSM database…", onSelect: () => {
          try { openDatabase(); } catch (e) { fail(e); }
        }}, {id: "folder", label: "Open a duckOSM folder…", onSelect: () => {
          const url = prompt("The URL of a duckOSM GeoParquet folder (with layers.json):");
          if (url) openFolder(url.trim(), defaultLook(), defaultMapLook()).catch(fail);
        }}, {id: "add", label: "Add a roadstyle layer…", onSelect: () => {
          const url = prompt("The URL of a roadstyle export (style.json):");
          if (url) addStyle(url.trim()).catch(fail);
        }}, {id: "look", label: "Style a layer with roadstyle…", onSelect: () => {
          const layer = pickLayer(); if (!layer) return;
          const url = prompt("The URL of a roadstyle look (rs.export_look):", defaultLook());
          if (url) styleLayer(layer, url.trim()).catch(fail);
        }}],
      });
    }
  },
  async handleUrlParameters(host, params) {
    app = app || host;
    for (const url of params.getAll("roadstyle")) await addStyle(url).catch(fail);
    for (const url of params.getAll("roadstyle_look")) {
      const look = await (await fetch(url)).json();
      styleFiles((n) => (n.startsWith("edges_") ? look : n === "boundary" ? look.boundary || null : null), "roadstyle look").catch(fail);
    }
    for (const url of params.getAll("mapstyle_look")) {
      const look = await (await fetch(url)).json();
      styleFiles((n) => look.files[n] || null, look.name).catch(fail);
    }
  },
  deactivate() {
    removeAll();
    unstyleAll();
    closeDatabases();
    if (disposeMenu) disposeMenu();
    disposeMenu = null;
  },
  getProjectState() {
    return {urls: added.map((a) => a.url)};
  },
  applyProjectState(host, state) {
    app = app || host;
    if (!state || !Array.isArray(state.urls)) return false;
    state.urls.filter((u) => !added.some((a) => a.url === u)).forEach((u) => addStyle(u).catch(fail));
  },
};

export default plugin;
