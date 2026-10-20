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

function closePopup() {
  if (popup) popup.remove();
  popup = null;
}

function showPopup(m, point, props) {
  closePopup();
  const rows = Object.entries(props).filter(([k]) => !k.startsWith("__")).map(([k, v]) =>
    `<tr><th style="text-align:left;padding:1px 8px 1px 0;font-weight:600">${k}</th><td>${v}</td></tr>`).join("");
  popup = document.createElement("div");
  popup.style.cssText = "position:absolute;z-index:5;background:#fff;color:#222;border-radius:6px;box-shadow:0 2px 10px rgba(0,0,0,.3);" +
    "padding:8px 10px;font:12px/1.4 system-ui,sans-serif;max-height:320px;overflow:auto;max-width:360px";
  popup.style.left = point.x + 12 + "px";
  popup.style.top = point.y + 12 + "px";
  popup.innerHTML = `<div style="text-align:right;cursor:pointer" title="Close">✕</div><table>${rows}</table>`;
  popup.firstChild.onclick = closePopup;
  m.getContainer().appendChild(popup);
}

// the road under the cursor: the roads tile layer of an added export (in simple mode its fill layer is unseen but kept for this)
function roadAt(m, point) {
  const ids = added.flatMap((a) => a.layerIds).filter((id) => m.getLayer(id) && m.getLayer(id).type === "line");
  return m.queryRenderedFeatures(point, {layers: ids}).find((f) => f.sourceLayer === "roads") || null;
}

function onClick(e) {
  const m = map();
  if (!m || (app.getActiveMapTool && app.getActiveMapTool() !== null)) return;     // a GeoLibre tool (Identify, ...) has the click
  const f = roadAt(m, e.point);
  if (f) showPopup(m, e.point, f.properties); else closePopup();
}

function onMove(e) {
  const m = map();
  if (!m) return;
  const f = roadAt(m, e.point);
  if (hover) m.setFeatureState(hover, {hover: false});
  hover = f && f.id != null ? {source: f.source, sourceLayer: "roads", id: f.id} : null;
  if (hover) m.setFeatureState(hover, {hover: true});
  m.getCanvas().style.cursor = f ? "pointer" : "";
}

async function addStyle(url) {
  const m = map();
  if (!m) throw new Error("roadstyle: GeoLibre has no MapLibre map (the plugin needs the MapLibre renderer)");
  const style = await (await fetch(url)).json();
  const prefix = `rs${added.length}-`;
  await loadSprite(m, style.sprite, prefix);
  const font = baseFont(m);
  const sourceIds = [], layerIds = [];
  for (const [id, src] of Object.entries(style.sources)) {
    m.addSource(prefix + id, src);
    sourceIds.push(prefix + id);
  }
  for (const l of style.layers) {
    const layout = {...(l.layout || {})};
    if (layout["text-font"] && font) layout["text-font"] = font;
    if (typeof layout["icon-image"] === "string") layout["icon-image"] = prefix + layout["icon-image"];
    m.addLayer({...l, id: prefix + l.id, source: prefix + l.source, layout});
    layerIds.push(prefix + l.id);
  }
  const name = (style.name || "roads") + " (roadstyle)";
  if (app.registerExternalNativeLayer) {
    app.registerExternalNativeLayer({
      id: prefix + "layer", name, type: "vector", nativeLayerIds: layerIds, sourceIds, opacity: 1, style: {}, metadata: {url},
      paintMode: "plugin",            // roadstyle paints these layers: GeoLibre's style editors do not apply
      paintBridge: {setVisibility: (v) => layerIds.forEach((id) => m.getLayer(id) && m.setLayoutProperty(id, "visibility", v ? "visible" : "none"))},
    });
  }
  if (!added.length) { m.on("click", onClick); m.on("mousemove", onMove); m.on("movestart", closePopup); }
  added.push({url, prefix, layerIds, sourceIds});
  if (style.center && style.zoom != null) m.jumpTo({center: style.center, zoom: style.zoom});
}

// ---- approach 2 (docs/design/style_export.md): roadstyle's simple look on a GeoLibre data layer (DuckDB, GeoParquet, GeoJSON with
// duckOSM columns). The look (rs.export_look: layers with no source, made in Python from a profile and a palette) goes on the data
// layer's own MapLibre source; GeoLibre's plain line goes unseen but stays, so Identify still finds the roads.
const styled = [];           // [{layer, ids}]

async function styleLayer(layerId, lookUrl) {
  const m = map();
  const src = "source-" + layerId, own = "layer-" + layerId + "-line";
  if (!m || !m.getSource(src)) throw new Error("roadstyle: this layer has no data source the plugin can read (" + src + ")");
  const look = await (await fetch(lookUrl)).json();
  const feats = (app.getLayerFeatures ? app.getLayerFeatures(layerId) : null);
  const props = ((feats && feats.features) || feats || []).map((f) => f.properties || {});
  const missing = (look.columns || []).filter((c) => !props.some((p) => c in p));
  // the positions the data has (a road without them is at position 0): only their layers are added
  const have = new Set(props.flatMap((p) => [p.casing_level, p.fill_level]).map((v) => (v == null ? 0 : Number(v))));
  const ids = m.getStyle().layers.map((l) => l.id);
  const before = ids[ids.indexOf(own) + 1];                         // right above GeoLibre's own line
  const added_ = [];
  for (const l of look.layers) {
    const pos = l.metadata && l.metadata["roadstyle:position"];
    if (pos != null && !have.has(pos)) continue;
    const id = "rslook-" + layerId + "-" + l.id;
    m.addLayer({...l, id, source: src}, before);
    added_.push(id);
  }
  const own0 = m.getLayer(own) && m.getPaintProperty(own, "line-color");
  ownColor[layerId] = typeof own0 === "string" ? own0 : "#3388ff";
  hideOwnLine(layerId);
  if (app.registerExternalNativeLayer) {
    app.registerExternalNativeLayer({
      id: "rslook-" + layerId, name: ((app.listLayers().find((l) => l.id === layerId) || {}).name || layerId) + " · roadstyle look" + (look.name ? " (" + look.name + ")" : ""), type: "vector",
      nativeLayerIds: added_, sourceIds: [], opacity: 1, style: {}, paintMode: "plugin", metadata: {layerId, lookUrl},
      paintBridge: {setVisibility: (v) => added_.forEach((id) => m.getLayer(id) && m.setLayoutProperty(id, "visibility", v ? "visible" : "none"))},
    });
  }
  styled.push({layer: layerId, ids: added_, lookUrl});
  if (missing.length) alert("roadstyle: the layer has no column " + missing.join(", ") + ": the look shows what it can (no column " +
                            "casing_level / fill_level: every road at one drawing position).");
}

// GeoLibre's own line of the data layer, unseen (a transparent colour), set through GeoLibre's style so that GeoLibre keeps it: it
// redraws its layers from its own store, and a paint set on the map alone would come back. GeoLibre's layer style has a line colour but
// no line opacity. The layer stays, so Identify still finds the roads. ``color``: the colour to put back (unstyleAll).
const ownColor = {};
function hideOwnLine(layerId, color = "rgba(0,0,0,0)") {
  const res = app.importLayerStyle ? app.importLayerStyle(layerId, JSON.stringify({version: 8, sources: {}, layers: [
    {id: "roads", type: "line", source: "roads", paint: {"line-color": color}}]})) : null;
  console.info("roadstyle: GeoLibre's line of layer", layerId, "->", color, JSON.stringify(res));
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
    if (app.unregisterExternalNativeLayer) app.unregisterExternalNativeLayer("rslook-" + s.layer);
    if (!m) continue;
    s.ids.forEach((id) => m.getLayer(id) && m.removeLayer(id));
    hideOwnLine(s.layer, ownColor[s.layer] || "#3388ff");
  }
}

function removeAll() {
  const m = map();
  closePopup();
  for (const a of added.splice(0)) {
    if (app.unregisterExternalNativeLayer) app.unregisterExternalNativeLayer(a.prefix + "layer");
    if (!m) continue;
    a.layerIds.slice().reverse().forEach((id) => m.getLayer(id) && m.removeLayer(id));
    a.sourceIds.forEach((id) => m.getSource(id) && m.removeSource(id));
  }
  if (m) { m.off("click", onClick); m.off("mousemove", onMove); m.off("movestart", closePopup); }
}

const fail = (e) => { console.error(e); alert(String(e.message || e)); };

// A duckOSM folder (duckosm export-gis --format geoparquet): GeoLibre opens many GeoParquet files from one link
// (?data=A&data=B), so the plugin makes that link from layers.json and adds roadstyle_look=<look URL>. After the
// page opens it again, the plugin styles every edges layer with the look.
const ORDER = {boundary: 0, nodes: 1, edges: 2}, MODES = {walking: 0, cycling: 1, driving: 2};
const defaultLook = () => (app.resolvePluginAssetUrl && app.resolvePluginAssetUrl(ID, "looks/default.json")) || "";

async function openFolder(folder, lookUrl) {
  const base = folder.replace(/layers\.json$/, "").replace(/\/?$/, "/");
  const res = await fetch(base + "layers.json");
  if (!res.ok) throw new Error("roadstyle: no layers.json in " + base + " (HTTP " + res.status + "). Write the folder with: duckosm export-gis --format geoparquet");
  const files = (await res.json()).layers.slice().sort((a, b) =>
    (ORDER[a.kind] ?? 9) - (ORDER[b.kind] ?? 9) || (MODES[a.mode] ?? 9) - (MODES[b.mode] ?? 9));
  const q = new URLSearchParams();
  files.forEach((f) => q.append("data", new URL(f.file, base).href));
  q.set("roadstyle_look", lookUrl);
  location.assign(location.origin + location.pathname + "?" + q.toString());
}

// Wait for GeoLibre to load the edges files of the link, then style each one.
async function styleEdges(lookUrl) {
  const want = new URLSearchParams(location.search).getAll("data")
    .map((u) => decodeURIComponent(new URL(u).pathname.split("/").pop()).replace(/\.(geo)?parquet$/i, ""))
    .filter((n) => n.startsWith("edges"));
  const t0 = Date.now();
  for (;;) {
    const have = (app.listLayers ? app.listLayers() : []).filter((l) => want.includes(l.name));
    if (have.length === want.length && have.every((l) => map() && map().getSource("source-" + l.id))) {
      for (const l of have) await styleLayer(l.id, lookUrl);
      return;
    }
    if (Date.now() - t0 > 600000) throw new Error("roadstyle: GeoLibre did not load " + want.join(", ") + " in 10 minutes; the look is not applied.");
    await new Promise((r) => setTimeout(r, 1000));
  }
}

export const plugin = {
  id: ID,
  name: "roadstyle",
  version: "0.3.0",
  urlParameterNames: ["roadstyle", "roadstyle_look"],
  activate(host) {
    app = host;
    if (app.registerToolbarMenu) {
      disposeMenu = app.registerToolbarMenu({
        id: "roadstyle-menu", label: "roadstyle",
        items: [{id: "folder", label: "Open a duckOSM folder…", onSelect: () => {
          const url = prompt("The URL of a duckOSM GeoParquet folder (with layers.json):");
          if (url) openFolder(url.trim(), defaultLook()).catch(fail);
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
    for (const url of params.getAll("roadstyle_look")) styleEdges(url).catch(fail);
  },
  deactivate() {
    removeAll();
    unstyleAll();
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
