// roadstyle for GeoLibre (docs/design/style_export.md, version 2): a roadstyle export (style.json + tiles + sprite, made by
// rs.export_style from a duckOSM GeoParquet file) as one layer over GeoLibre's own base map. The style is roadstyle's, made in
// Python; this plugin only adds it to GeoLibre's map and does the interaction: one row in the Layers panel, a popup with the
// road's data on click, the hover highlight. Open one with the menu "roadstyle" or the link parameter ?roadstyle=<style.json URL>.

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

export const plugin = {
  id: ID,
  name: "roadstyle",
  version: "0.1.0",
  urlParameterNames: ["roadstyle"],
  activate(host) {
    app = host;
    if (app.registerToolbarMenu) {
      disposeMenu = app.registerToolbarMenu({
        id: "roadstyle-menu", label: "roadstyle",
        items: [{id: "add", label: "Add a roadstyle layer…", onSelect: () => {
          const url = prompt("The URL of a roadstyle export (style.json):");
          if (url) addStyle(url.trim()).catch(fail);
        }}],
      });
    }
  },
  async handleUrlParameters(host, params) {
    app = app || host;
    for (const url of params.getAll("roadstyle")) await addStyle(url).catch(fail);
  },
  deactivate() {
    removeAll();
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
